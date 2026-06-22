//! Voice calls on the sans-IO `rtc` crate — the server is the WebRTC peer in the
//! middle (SFU-lite). Each participant gets a server-side `RTCPeerConnection`
//! driven on its own UDP socket in a spawned task; that task forwards each
//! decrypted Opus RTP packet to the other participants in the same call (and taps
//! the recorder). Signaling rides the same `/ws` frames as before, so the
//! frontend is unchanged.
//!
//! Why sans-IO: we own the socket and the poll loop, which (a) makes the
//! forwarding logic unit-testable in-process (see `peer.rs`) and (b) gives full
//! ICE control — we advertise our public host candidate explicitly with
//! `add_local_candidate` and accept the client's trickle with
//! `add_remote_candidate`, and because we feed `handle_read` the real source
//! address, peer-reflexive just works. No nat_1to1 / UDP-mux hacks needed.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use bytes::{Bytes, BytesMut};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};
use ulid::Ulid;

use rtc::peer_connection::configuration::RTCConfigurationBuilder;
use rtc::peer_connection::event::RTCPeerConnectionEvent;
use rtc::peer_connection::message::RTCMessage;
use rtc::peer_connection::sdp::RTCSessionDescription;
use rtc::peer_connection::state::RTCPeerConnectionState;
use rtc::peer_connection::transport::{
    CandidateConfig, CandidateHostConfig, RTCIceCandidate, RTCIceCandidateInit, RTCIceServer,
};
use rtc::peer_connection::RTCPeerConnection;
use rtc::peer_connection::RTCPeerConnectionBuilder;
use rtc::rtp;
use rtc::rtp_transceiver::RTCRtpSenderId;
use rtc::sansio::Protocol;
use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};

use crate::models::{CallParticipant, Outbound, Signal};
use mixer::Mixer;
use peer::{audio_media_engine, opus_track, parse_mid_layout};
use recorder::Recorder;

mod mixer;
mod peer;
mod recorder;

const MAX_DATAGRAM: usize = 2000;
/// Pre-allocated outbound audio tracks per participant — one per OTHER source, so
/// each speaker lands on its own SSRC/track and the browser doesn't drop a late
/// joiner's stream. Caps a call at FORWARD_TRACKS + 1 participants (flat, no
/// renegotiation).
const FORWARD_TRACKS: usize = 8;
/// Cap how long a poll-timeout can sleep so commands/sockets stay responsive.
const MAX_IDLE: Duration = Duration::from_millis(50);

/// Commands a driver task accepts (from signaling and from peers' RTP).
enum Cmd {
    /// The participant's SDP answer to our offer.
    Answer(String),
    /// A trickled ICE candidate from the participant (JSON `RTCIceCandidateInit`).
    RemoteIce(String),
    /// Audio from another participant to write down to this one, tagged with the
    /// source so the receiver puts each source on its own outbound track.
    Forward {
        from: String,
        pkt: Arc<rtp::packet::Packet>,
    },
    /// Tear this leg down.
    Close,
}

/// How the call sees a participant: identity + a handle to its driver task.
struct Member {
    id: String,
    name: String,
    tx: mpsc::UnboundedSender<Cmd>,
}

/// A live call in one room.
struct Call {
    id: String,
    room_id: String,
    signal: broadcast::Sender<Signal>,
    members: Mutex<HashMap<String, Member>>,
    recorder: Recorder,
    mixer: Mixer,
}

impl Call {
    fn other_senders(&self, except: &str) -> Vec<mpsc::UnboundedSender<Cmd>> {
        self.members
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.id != except)
            .map(|m| m.tx.clone())
            .collect()
    }

    fn roster(&self) -> Vec<CallParticipant> {
        self.members
            .lock()
            .unwrap()
            .values()
            .map(|m| CallParticipant {
                id: m.id.clone(),
                name: m.name.clone(),
            })
            .collect()
    }

    fn emit(&self, target: Option<String>, exclude: Option<String>, frame: Outbound) {
        let _ = self.signal.send(Signal {
            room_id: self.room_id.clone(),
            target,
            exclude,
            all_employees: false,
            frame,
        });
    }
}

/// Owns every live call and the call media configuration.
pub struct CallRegistry {
    stun: Vec<String>,
    /// Public IP(s) to advertise as our host candidate (NAT/DMZ). First is used.
    /// Mutable so a background task can fill/refresh it via an external IP service
    /// when `ZENITHAR_PUBLIC_IP` isn't set.
    public_ips: Mutex<Vec<String>>,
    /// Fixed UDP port range to bind media on (forwarded 1:1 in the router). None
    /// = ephemeral. We allocate one socket per participant from this range.
    udp_ports: Option<(u16, u16)>,
    next_port: Mutex<u16>,
    signal: broadcast::Sender<Signal>,
    db: sqlx::SqlitePool,
    recordings_dir: PathBuf,
    calls: Mutex<HashMap<String, Arc<Call>>>, // by room_id
}

impl CallRegistry {
    pub fn new(
        stun: Vec<String>,
        public_ips: Vec<String>,
        udp_ports: Option<(u16, u16)>,
        signal: broadcast::Sender<Signal>,
        db: sqlx::SqlitePool,
        recordings_dir: PathBuf,
    ) -> Result<Self> {
        info!(?public_ips, ?udp_ports, ?stun, "call media config");
        let next_port = udp_ports.map(|(lo, _)| lo).unwrap_or(0);
        Ok(Self {
            stun,
            public_ips: Mutex::new(public_ips),
            udp_ports,
            next_port: Mutex::new(next_port),
            signal,
            db,
            recordings_dir,
            calls: Mutex::new(HashMap::new()),
        })
    }

    /// Replace the advertised public IP(s) (used by the auto-discovery task when
    /// `ZENITHAR_PUBLIC_IP` is unset). Affects calls started after this point.
    pub fn set_public_ips(&self, ips: Vec<String>) {
        let mut cur = self.public_ips.lock().unwrap();
        if *cur != ips {
            info!(?ips, "public IP updated (auto-discovered)");
            *cur = ips;
        }
    }

    /// Whether a public IP is currently known (else calls fall back to 127.0.0.1).
    pub fn has_public_ip(&self) -> bool {
        !self.public_ips.lock().unwrap().is_empty()
    }

    fn ice_servers(&self) -> Vec<RTCIceServer> {
        if self.stun.is_empty() {
            vec![]
        } else {
            vec![RTCIceServer {
                urls: self.stun.clone(),
                ..Default::default()
            }]
        }
    }

    /// Bind a UDP socket on `0.0.0.0` — a port from the configured range (so a DMZ
    /// can forward exactly that range), else an ephemeral one.
    async fn bind_media_socket(&self) -> Result<UdpSocket> {
        let Some((lo, hi)) = self.udp_ports else {
            return Ok(UdpSocket::bind("0.0.0.0:0").await?);
        };
        let start = {
            let mut p = self.next_port.lock().unwrap();
            let cur = (*p).clamp(lo, hi);
            *p = if cur >= hi { lo } else { cur + 1 };
            cur
        };
        for port in (start..=hi).chain(lo..start) {
            if let Ok(s) = UdpSocket::bind(("0.0.0.0", port)).await {
                return Ok(s);
            }
        }
        anyhow::bail!("no free UDP port in {lo}-{hi}")
    }

    fn find(&self, call_id: &str) -> Option<Arc<Call>> {
        self.calls
            .lock()
            .unwrap()
            .values()
            .find(|c| c.id == call_id)
            .cloned()
    }

    /// Start (or join) the call in `room_id`. Builds the participant's server
    /// PeerConnection, returns the SDP offer to send them, and rings the room.
    #[tracing::instrument(skip_all, fields(room = %room_id, participant = %principal_id))]
    pub async fn join(
        self: &Arc<Self>,
        room_id: &str,
        principal_id: &str,
        principal_name: &str,
    ) -> Result<(String, String)> {
        let (call, created) = {
            let mut map = self.calls.lock().unwrap();
            if let Some(c) = map.get(room_id) {
                (c.clone(), false)
            } else {
                let id = Ulid::new().to_string();
                let call = Arc::new(Call {
                    recorder: Recorder::new(self.recordings_dir.clone(), &id),
                    mixer: Mixer::new(self.recordings_dir.clone(), &id),
                    id,
                    room_id: room_id.to_string(),
                    signal: self.signal.clone(),
                    members: Mutex::new(HashMap::new()),
                });
                map.insert(room_id.to_string(), call.clone());
                (call, true)
            }
        };
        if created {
            if let Err(e) = crate::db::insert_call(
                &self.db,
                &call.id,
                room_id,
                principal_id,
                crate::now_millis(),
            )
            .await
            {
                warn!(error = %e, "failed to log call start");
            }
        }

        let socket = self.bind_media_socket().await?;
        let local_addr = socket.local_addr()?;

        // Build the server-side PeerConnection (we are the offerer; we don't set a
        // DTLS role so the browser answerer becomes DTLS client and we DTLS server).
        let mut pc = RTCPeerConnectionBuilder::new()
            .with_configuration(
                RTCConfigurationBuilder::new()
                    .with_ice_servers(self.ice_servers())
                    .build(),
            )
            .with_media_engine(audio_media_engine()?)
            .build()?;

        // Pre-allocate one outbound track per potential other participant, each
        // with its own SSRC/stream, so every speaker is forwarded on a distinct
        // track (mixing several sources onto one would make the browser drop all
        // but the first). The first track's transceiver also carries the inbound
        // direction — the browser attaches its mic there.
        let mut senders = Vec::with_capacity(FORWARD_TRACKS);
        let mut send_ssrcs = Vec::with_capacity(FORWARD_TRACKS);
        for i in 0..FORWARD_TRACKS {
            let ssrc = u32::from_le_bytes(crate::auth::random_bytes::<4>());
            let sid = pc.add_track(opus_track(
                &format!("zenithar-{i}"),
                &format!("voice-{principal_id}-{i}"),
                ssrc,
            ))?;
            senders.push(sid);
            send_ssrcs.push(ssrc);
        }

        // Advertise our reachable host candidate (public IP in prod, loopback in
        // dev) on the bound port. It rides in the offer SDP, so the client gets it
        // without a separate trickle.
        //
        // IMPORTANT: the socket is bound to 0.0.0.0 (to catch the DMZ-forwarded
        // traffic on any interface), but the candidate — and therefore the
        // `local_addr` we feed `handle_read` — must be this ADVERTISED address.
        // The ICE agent matches incoming checks to a known local candidate by that
        // address; feeding it `0.0.0.0:port` makes it discard every check
        // ("not a valid local candidate").
        let adv_ip = self
            .public_ips
            .lock()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let adv_addr: SocketAddr = format!("{adv_ip}:{}", local_addr.port())
            .parse()
            .map_err(|e| anyhow::anyhow!("bad advertised media addr {adv_ip}: {e}"))?;
        pc.add_local_candidate(host_candidate(&adv_ip, local_addr.port())?)?;
        info!(
            participant = %principal_id,
            advertised = %adv_addr,
            bound = %local_addr,
            "call leg media socket"
        );

        let offer = pc.create_offer(None)?;
        let offer_sdp = offer.sdp.clone();
        pc.set_local_description(offer)?;

        // Map slot i (the i-th add_track) → its m-line MID, and grab the negotiated
        // MID extension id, both parsed from our own offer. We stamp these on
        // forwarded packets so the receiver's BUNDLE demux routes each source to
        // its own track (see SDES_MID_URI in peer.rs).
        let (mid_ext_id, slot_mids) = parse_mid_layout(&offer_sdp);
        match mid_ext_id {
            Some(id) if slot_mids.len() >= FORWARD_TRACKS => {
                info!(
                    participant = %principal_id,
                    mid_ext_id = id,
                    slots = slot_mids.len(),
                    "MID stamping active (group-call demux)"
                );
            }
            _ => {
                warn!(
                    participant = %principal_id,
                    ?mid_ext_id,
                    mids = slot_mids.len(),
                    tracks = FORWARD_TRACKS,
                    "MID layout incomplete — group-call audio demux may fail"
                );
            }
        }

        // Register the participant and spawn its driver.
        let (tx, rx) = mpsc::unbounded_channel();
        call.members.lock().unwrap().insert(
            principal_id.to_string(),
            Member {
                id: principal_id.to_string(),
                name: principal_name.to_string(),
                tx,
            },
        );
        tokio::spawn(
            Driver {
                reg: Arc::downgrade(self),
                call: call.clone(),
                my_id: principal_id.to_string(),
                socket,
                // Feed the ICE agent the advertised candidate address, NOT the
                // 0.0.0.0 bind addr (see above).
                local_addr: adv_addr,
                pc,
                senders,
                send_ssrcs,
                slots: HashMap::new(),
                mid_ext_id,
                slot_mids,
                rx,
                logged_peer: false,
                logged_rtp_in: false,
                logged_rtp_out: false,
            }
            .run(),
        );

        call.emit(
            None,
            Some(principal_id.to_string()),
            Outbound::CallRinging {
                call_id: call.id.clone(),
                room_id: room_id.to_string(),
                from: principal_id.to_string(),
                from_name: principal_name.to_string(),
            },
        );
        call.emit(
            None,
            None,
            Outbound::CallState {
                call_id: call.id.clone(),
                participants: call.roster(),
            },
        );

        Ok((call.id.clone(), offer_sdp))
    }

    pub async fn answer(&self, call_id: &str, principal_id: &str, sdp: String) -> Result<()> {
        self.send_cmd(call_id, principal_id, Cmd::Answer(sdp));
        Ok(())
    }

    pub async fn ice(&self, call_id: &str, principal_id: &str, candidate: String) -> Result<()> {
        self.send_cmd(call_id, principal_id, Cmd::RemoteIce(candidate));
        Ok(())
    }

    pub async fn leave(&self, call_id: &str, principal_id: &str) {
        self.send_cmd(call_id, principal_id, Cmd::Close);
    }

    fn send_cmd(&self, call_id: &str, principal_id: &str, cmd: Cmd) {
        if let Some(call) = self.find(call_id) {
            if let Some(m) = call.members.lock().unwrap().get(principal_id) {
                let _ = m.tx.send(cmd);
            }
        }
    }

    /// A driver task has exited (left, dropped, or its PC failed). Remove the
    /// participant. A call needs at least two people, so once it drops below that
    /// the call is over: tear down any lone remaining leg and tell the room it
    /// ended — this is what makes a 1:1 hangup end BOTH sides, not just the one
    /// that left. With 2+ still in, it's a group call: just broadcast the roster.
    /// Idempotent (a second exit finds the call already gone).
    async fn member_gone(&self, call_id: &str, principal_id: &str) {
        let Some(call) = self.find(call_id) else {
            return;
        };
        let remaining: Vec<mpsc::UnboundedSender<Cmd>> = {
            let mut map = call.members.lock().unwrap();
            map.remove(principal_id);
            map.values().map(|m| m.tx.clone()).collect()
        };
        if remaining.len() >= 2 {
            call.emit(
                None,
                None,
                Outbound::CallState {
                    call_id: call.id.clone(),
                    participants: call.roster(),
                },
            );
            return;
        }

        // Fewer than two left → end the call.
        self.calls.lock().unwrap().remove(&call.room_id);
        for tx in &remaining {
            let _ = tx.send(Cmd::Close); // close the lone straggler's server leg
        }
        let recorded = call.recorder.finalize();
        let mixed = call.mixer.finalize();
        if let Err(e) = crate::db::end_call(&self.db, &call.id, crate::now_millis()).await {
            warn!(error = %e, "failed to log call end");
        }
        if recorded || mixed {
            if let Err(e) = crate::db::set_call_recording(&self.db, &call.id, &call.id).await {
                warn!(error = %e, "failed to record recording_id");
            }
        }
        call.emit(
            None,
            None,
            Outbound::CallEnded {
                call_id: call.id.clone(),
            },
        );
    }
}

/// Owns one participant's PeerConnection + socket and drives the sans-IO loop.
struct Driver {
    reg: Weak<CallRegistry>,
    call: Arc<Call>,
    my_id: String,
    socket: UdpSocket,
    local_addr: SocketAddr,
    pc: RTCPeerConnection,
    /// Pre-allocated outbound tracks (one per remote source) + their SSRCs. A
    /// forwarded packet carries the *source's* SSRC, so we rewrite it to the
    /// slot's SSRC before write_rtp or the browser drops it as an unknown stream.
    senders: Vec<RTCRtpSenderId>,
    send_ssrcs: Vec<u32>,
    /// Which outbound track slot each source participant is forwarded on.
    slots: HashMap<String, usize>,
    /// Negotiated MID header-extension id, and the MID per slot (= per outbound
    /// track). Stamped on each forwarded packet so the browser demuxes it onto the
    /// right `<audio>` under BUNDLE; without it, group calls don't route audio.
    mid_ext_id: Option<u8>,
    slot_mids: Vec<String>,
    rx: mpsc::UnboundedReceiver<Cmd>,
    /// One-shot: log the first inbound datagram's source (the client's real,
    /// post-NAT address) so a deploy can confirm checks are arriving + from where.
    logged_peer: bool,
    /// One-shot: first decrypted RTP we got FROM this participant (their mic).
    logged_rtp_in: bool,
    /// One-shot: first RTP we wrote TOWARD this participant (another's audio).
    logged_rtp_out: bool,
}

impl Driver {
    /// The outbound track slot a source is forwarded on — assigning a free one on
    /// first sight. `None` once all slots are taken (call past its flat capacity);
    /// that source's audio is then dropped (rare: needs > FORWARD_TRACKS others).
    fn slot_for(&mut self, source: &str) -> Option<usize> {
        if let Some(&s) = self.slots.get(source) {
            return Some(s);
        }
        let s = self.slots.len();
        if s >= self.senders.len() {
            debug!(participant = %self.my_id, "call full — dropping a participant's audio");
            return None;
        }
        self.slots.insert(source.to_string(), s);
        Some(s)
    }

    #[tracing::instrument(skip_all, fields(call = %self.call.id, participant = %self.my_id))]
    async fn run(mut self) {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        'drive: loop {
            // 1. Flush outgoing UDP.
            while let Some(msg) = self.pc.poll_write() {
                let _ = self
                    .socket
                    .send_to(&msg.message, msg.transport.peer_addr)
                    .await;
            }

            // 2. State events.
            let mut dead = false;
            while let Some(ev) = self.pc.poll_event() {
                if let RTCPeerConnectionEvent::OnConnectionStateChangeEvent(s) = ev {
                    debug!(participant = %self.my_id, state = %s, "pc state");
                    // `Disconnected` is transient (ICE may recover) — don't tear
                    // the leg down on it, or a brief blip ends the whole call.
                    if matches!(
                        s,
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed
                    ) {
                        dead = true;
                    }
                }
            }

            // 3. Inbound RTP → record + forward to the other participants.
            while let Some(m) = self.pc.poll_read() {
                if let RTCMessage::RtpPacket(_, pkt) = m {
                    let others = self.call.other_senders(&self.my_id);
                    if !self.logged_rtp_in {
                        self.logged_rtp_in = true;
                        info!(
                            participant = %self.my_id,
                            forwarding_to = others.len(),
                            "first RTP in (got this participant's audio)"
                        );
                    }
                    self.call.recorder.write(&self.my_id, &pkt);
                    self.call
                        .mixer
                        .add(&self.my_id, pkt.header.timestamp, &pkt.payload);
                    let pkt = Arc::new(pkt);
                    for tx in others {
                        let _ = tx.send(Cmd::Forward {
                            from: self.my_id.clone(),
                            pkt: pkt.clone(),
                        });
                    }
                }
            }

            if dead {
                break 'drive;
            }

            // 4. Sleep until the next timer or some I/O.
            let eto = self
                .pc
                .poll_timeout()
                .unwrap_or_else(|| Instant::now() + MAX_IDLE);
            let delay = eto
                .checked_duration_since(Instant::now())
                .unwrap_or_default()
                .min(MAX_IDLE);
            let timer = tokio::time::sleep(delay);
            tokio::pin!(timer);

            tokio::select! {
                _ = &mut timer => {
                    let _ = self.pc.handle_timeout(Instant::now());
                }
                r = self.socket.recv_from(&mut buf) => {
                    if let Ok((n, peer)) = r {
                        if !self.logged_peer {
                            self.logged_peer = true;
                            info!(
                                participant = %self.my_id,
                                from = %peer,
                                local = %self.local_addr,
                                "first inbound media datagram"
                            );
                        }
                        let _ = self.pc.handle_read(TaggedBytesMut {
                            now: Instant::now(),
                            transport: TransportContext {
                                local_addr: self.local_addr,
                                peer_addr: peer,
                                ecn: None,
                                transport_protocol: TransportProtocol::UDP,
                            },
                            message: BytesMut::from(&buf[..n]),
                        });
                    }
                }
                cmd = self.rx.recv() => {
                    match cmd {
                        Some(Cmd::Answer(sdp)) => {
                            match RTCSessionDescription::answer(sdp) {
                                Ok(a) => { let _ = self.pc.set_remote_description(a); }
                                Err(e) => debug!(error = %e, "bad answer sdp"),
                            }
                        }
                        Some(Cmd::RemoteIce(c)) => {
                            match serde_json::from_str::<RTCIceCandidateInit>(&c) {
                                Ok(init) => { let _ = self.pc.add_remote_candidate(init); }
                                Err(e) => debug!(error = %e, "bad remote candidate"),
                            }
                        }
                        Some(Cmd::Forward { from, pkt }) => {
                            if let Some(slot) = self.slot_for(&from) {
                                let ssrc = self.send_ssrcs[slot];
                                if let Some(mut sender) = self.pc.rtp_sender(self.senders[slot]) {
                                    if !self.logged_rtp_out {
                                        self.logged_rtp_out = true;
                                        info!(participant = %self.my_id, "first RTP out (writing audio toward this participant)");
                                    }
                                    // Re-stamp the source's SSRC with this slot's
                                    // track SSRC so the browser accepts the stream.
                                    let mut p = (*pkt).clone();
                                    p.header.ssrc = ssrc;
                                    // Stamp the slot's MID so the browser demuxes
                                    // this onto the right track under BUNDLE.
                                    if let (Some(id), Some(mid)) =
                                        (self.mid_ext_id, self.slot_mids.get(slot))
                                    {
                                        p.header.extension = true;
                                        p.header.extension_profile =
                                            rtp::header::EXTENSION_PROFILE_ONE_BYTE;
                                        let _ = p
                                            .header
                                            .set_extension(id, Bytes::copy_from_slice(mid.as_bytes()));
                                    }
                                    let _ = sender.write_rtp(p);
                                }
                            }
                        }
                        Some(Cmd::Close) | None => break 'drive,
                    }
                }
            }
        }

        let _ = self.pc.close();
        if let Some(reg) = self.reg.upgrade() {
            reg.member_gone(&self.call.id, &self.my_id).await;
        }
    }
}

/// Build an ICE host candidate (as `RTCIceCandidateInit`) for `ip:port`.
/// Ask an external echo service for our public IP (plain-HTTP so it works with
/// our TLS-less reqwest). Returns the IP only if the body parses as one.
pub async fn discover_public_ip(service: &str) -> Option<String> {
    let resp = reqwest::Client::new()
        .get(service)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    let body = resp.bytes().await.ok()?;
    let ip = String::from_utf8_lossy(&body);
    let ip = ip.trim();
    ip.parse::<std::net::IpAddr>().ok()?;
    Some(ip.to_string())
}

fn host_candidate(ip: &str, port: u16) -> Result<RTCIceCandidateInit> {
    let cand = CandidateHostConfig {
        base_config: CandidateConfig {
            network: "udp".to_owned(),
            address: ip.to_owned(),
            port,
            component: 1,
            ..Default::default()
        },
        ..Default::default()
    }
    .new_candidate_host()?;
    Ok(RTCIceCandidate::from(&cand).to_json()?)
}
