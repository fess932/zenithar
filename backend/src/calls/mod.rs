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
use bytes::BytesMut;
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
use peer::{audio_media_engine, opus_track};
use recorder::Recorder;

mod peer;
mod recorder;

const MAX_DATAGRAM: usize = 2000;
/// Cap how long a poll-timeout can sleep so commands/sockets stay responsive.
const MAX_IDLE: Duration = Duration::from_millis(50);

/// Commands a driver task accepts (from signaling and from peers' RTP).
enum Cmd {
    /// The participant's SDP answer to our offer.
    Answer(String),
    /// A trickled ICE candidate from the participant (JSON `RTCIceCandidateInit`).
    RemoteIce(String),
    /// Audio from another participant to write down to this one.
    Forward(Arc<rtp::packet::Packet>),
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
            frame,
        });
    }
}

/// Owns every live call and the call media configuration.
pub struct CallRegistry {
    stun: Vec<String>,
    /// Public IP(s) to advertise as our host candidate (NAT/DMZ). First is used.
    public_ips: Vec<String>,
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
            public_ips,
            udp_ports,
            next_port: Mutex::new(next_port),
            signal,
            db,
            recordings_dir,
            calls: Mutex::new(HashMap::new()),
        })
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
            if let Err(e) =
                crate::db::insert_call(&self.db, &call.id, room_id, principal_id, crate::now_millis())
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

        // The track this participant *receives* (everyone else's audio).
        let ssrc = u32::from_le_bytes(crate::auth::random_bytes::<4>());
        let sender_id = pc.add_track(opus_track("zenithar", &format!("voice-{principal_id}"), ssrc))?;

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
                sender_id,
                send_ssrc: ssrc,
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
    /// participant; when the call empties, finalize the recording, log the end,
    /// and tell the room. Idempotent.
    async fn member_gone(&self, call_id: &str, principal_id: &str) {
        let Some(call) = self.find(call_id) else {
            return;
        };
        let now_empty = {
            let mut map = call.members.lock().unwrap();
            map.remove(principal_id);
            map.is_empty()
        };
        if now_empty {
            self.calls.lock().unwrap().remove(&call.room_id);
            let recorded = call.recorder.finalize();
            if let Err(e) = crate::db::end_call(&self.db, &call.id, crate::now_millis()).await {
                warn!(error = %e, "failed to log call end");
            }
            if recorded {
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
        } else {
            call.emit(
                None,
                None,
                Outbound::CallState {
                    call_id: call.id.clone(),
                    participants: call.roster(),
                },
            );
        }
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
    sender_id: RTCRtpSenderId,
    /// SSRC of THIS leg's outbound track. Forwarded packets (which carry the
    /// *source* participant's SSRC) must be rewritten to this before write_rtp,
    /// or the browser drops them as belonging to an unknown stream.
    send_ssrc: u32,
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
    async fn run(mut self) {
        let mut buf = vec![0u8; MAX_DATAGRAM];
        'drive: loop {
            // 1. Flush outgoing UDP.
            while let Some(msg) = self.pc.poll_write() {
                let _ = self.socket.send_to(&msg.message, msg.transport.peer_addr).await;
            }

            // 2. State events.
            let mut dead = false;
            while let Some(ev) = self.pc.poll_event() {
                if let RTCPeerConnectionEvent::OnConnectionStateChangeEvent(s) = ev {
                    debug!(participant = %self.my_id, state = %s, "pc state");
                    if matches!(
                        s,
                        RTCPeerConnectionState::Failed
                            | RTCPeerConnectionState::Disconnected
                            | RTCPeerConnectionState::Closed
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
                    let pkt = Arc::new(pkt);
                    for tx in others {
                        let _ = tx.send(Cmd::Forward(pkt.clone()));
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
                        Some(Cmd::Forward(pkt)) => {
                            if let Some(mut sender) = self.pc.rtp_sender(self.sender_id) {
                                if !self.logged_rtp_out {
                                    self.logged_rtp_out = true;
                                    info!(participant = %self.my_id, "first RTP out (writing audio toward this participant)");
                                }
                                // Re-stamp the source's SSRC with this leg's track
                                // SSRC so the browser accepts it as its stream.
                                let mut p = (*pkt).clone();
                                p.header.ssrc = self.send_ssrc;
                                let _ = sender.write_rtp(p);
                            } else if !self.logged_rtp_out {
                                self.logged_rtp_out = true;
                                warn!(participant = %self.my_id, "no rtp_sender to forward into — audio won't reach this participant");
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
