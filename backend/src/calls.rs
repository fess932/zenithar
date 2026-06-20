//! Voice calls — the server is the WebRTC peer in the middle (SFU-lite).
//!
//! Every browser negotiates exactly **one** `RTCPeerConnection` with the server.
//! The server terminates DTLS/SRTP, so it holds each participant's decrypted Opus
//! RTP and simply forwards every packet to the other participants in the same
//! call. Because the media already passes through here, Phase 5 (server-side
//! recording) is a tap on the forward loop — not a second media path.
//!
//! Topology per call (1:1 today, extends to N without a protocol change):
//! ```text
//! browser A ─DTLS/SRTP─▶ RTCPeerConnection A ─┐
//!                                             ├─ forward RTP ─▶ the other peers
//! browser B ─DTLS/SRTP─▶ RTCPeerConnection B ─┘   (+ Phase 5: write to .ogg)
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use anyhow::{Context, Result};
use tokio::sync::broadcast;
use tracing::{debug, warn};
use ulid::Ulid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice::udp_mux::{UDPMuxDefault, UDPMuxParams};
use webrtc::ice::udp_network::UDPNetwork;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::track::track_remote::TrackRemote;

use crate::models::{CallParticipant, Outbound, Signal};
use crate::recorder::Recorder;

/// One person's leg of a call: their server-side PeerConnection plus the local
/// track that carries *the other participants'* audio down to them.
struct Participant {
    id: String,
    name: String,
    pc: Arc<RTCPeerConnection>,
    /// Audio we write toward this participant (fed from everyone else's RTP).
    send_track: Arc<TrackLocalStaticRTP>,
}

/// A live call in one room. One per room for now.
struct Call {
    id: String,
    room_id: String,
    signal: broadcast::Sender<Signal>,
    participants: Mutex<HashMap<String, Arc<Participant>>>,
    /// Server-side tap of the forwarded Opus (Phase 5).
    recorder: Recorder,
}

impl Call {
    /// Snapshot of every other participant's send-track (so the per-packet
    /// forward loop never holds the lock across an `.await`).
    fn other_send_tracks(&self, except: &str) -> Vec<Arc<TrackLocalStaticRTP>> {
        self.participants
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.id != except)
            .map(|p| p.send_track.clone())
            .collect()
    }

    fn roster(&self) -> Vec<CallParticipant> {
        self.participants
            .lock()
            .unwrap()
            .values()
            .map(|p| CallParticipant {
                id: p.id.clone(),
                name: p.name.clone(),
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

/// Owns every live call and the shared WebRTC `API` (media engine + interceptors).
pub struct CallRegistry {
    api: API,
    stun: Vec<String>,
    signal: broadcast::Sender<Signal>,
    db: sqlx::SqlitePool,
    /// Where per-call Ogg/Opus recordings are written.
    recordings_dir: PathBuf,
    calls: Mutex<HashMap<String, Arc<Call>>>, // keyed by room_id
}

impl CallRegistry {
    pub fn new(
        stun: Vec<String>,
        public_ips: Vec<String>,
        media_port: Option<u16>,
        signal: broadcast::Sender<Signal>,
        db: sqlx::SqlitePool,
        recordings_dir: PathBuf,
    ) -> Result<Self> {
        let mut media = MediaEngine::default();
        media.register_default_codecs()?;
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media)?;

        // Behind NAT (typical self-host: cloud 1:1 NAT or a home server / DMZ),
        // the only addresses the OS sees are private, so a remote browser can't
        // connect. Advertise the public IP (NAT 1:1) as a host candidate without
        // depending on STUN (often blocked anyway). This is what an EXTERNAL
        // caller uses to reach the media path. (A caller on the *same LAN* as the
        // server can't use it — pinging your own router's public IP from inside
        // needs NAT hairpin, which most routers don't do — so test calls from a
        // different network, e.g. mobile data. webrtc-rs 0.17's Srflx mapping mode,
        // which would also keep the private candidate, doesn't trickle candidates
        // reliably here, so we use Host.)
        let mut setting = SettingEngine::default();
        if !public_ips.is_empty() {
            setting.set_nat_1to1_ips(public_ips, RTCIceCandidateType::Host);
        }
        // All call media over ONE fixed UDP port via a mux, bound to 0.0.0.0 so it
        // receives on every interface (key on a multi-homed / host-networked box:
        // forwarded UDP arriving on the real NIC is caught regardless of which
        // interface ICE would otherwise pick). One port to forward in the DMZ, and
        // it sidesteps the per-interface ephemeral binding that failed to gather
        // here. Many PeerConnections demux over this single socket by ufrag.
        if let Some(port) = media_port {
            let std_sock = std::net::UdpSocket::bind(("0.0.0.0", port))
                .with_context(|| format!("bind media UDP 0.0.0.0:{port}"))?;
            std_sock.set_nonblocking(true)?;
            let sock = tokio::net::UdpSocket::from_std(std_sock)?;
            let mux = UDPMuxDefault::new(UDPMuxParams::new(sock));
            setting.set_udp_network(UDPNetwork::Muxed(mux));
        }

        let api = APIBuilder::new()
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting)
            .build();
        Ok(Self {
            api,
            stun,
            signal,
            db,
            recordings_dir,
            calls: Mutex::new(HashMap::new()),
        })
    }

    fn ice_config(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: if self.stun.is_empty() {
                vec![]
            } else {
                vec![RTCIceServer {
                    urls: self.stun.clone(),
                    ..Default::default()
                }]
            },
            ..Default::default()
        }
    }

    fn find(&self, call_id: &str) -> Option<Arc<Call>> {
        self.calls
            .lock()
            .unwrap()
            .values()
            .find(|c| c.id == call_id)
            .cloned()
    }

    /// Start (or join) the call in `room_id` for this principal. Builds their
    /// server PeerConnection, returns the SDP offer to send back to them, and
    /// rings the rest of the room.
    pub async fn join(
        self: &Arc<Self>,
        room_id: &str,
        principal_id: &str,
        principal_name: &str,
    ) -> Result<(String, String)> {
        // Get or create the room's call.
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
                    participants: Mutex::new(HashMap::new()),
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

        // Build this participant's PeerConnection.
        let pc = Arc::new(self.api.new_peer_connection(self.ice_config()).await?);

        // Robustness (Phase 7): if the peer's connection drops (tab closed,
        // network lost, WS gone) without a clean `call-leave`, tear their leg
        // down so the call doesn't strand. We ignore `Closed` (that's our own
        // `leave` calling `pc.close()`), reacting only to network-level loss.
        {
            let weak_reg = Arc::downgrade(self);
            let call_id = call.id.clone();
            let pid = principal_id.to_string();
            pc.on_peer_connection_state_change(Box::new(move |s| {
                let weak_reg = weak_reg.clone();
                let call_id = call_id.clone();
                let pid = pid.clone();
                Box::pin(async move {
                    if matches!(
                        s,
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected
                    ) {
                        if let Some(reg) = weak_reg.upgrade() {
                            reg.leave(&call_id, &pid).await;
                        }
                    }
                })
            }));
        }

        // The local track this participant will *receive* (others' audio mixed in).
        let send_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                clock_rate: 48000,
                channels: 2,
                ..Default::default()
            },
            "audio".to_owned(),
            format!("zenithar-{principal_id}"),
        ));
        let rtp_sender = pc
            .add_track(Arc::clone(&send_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;
        // Drain the sender's RTCP so interceptors (NACK/TWCC) run; ends on close.
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1500];
            while rtp_sender.read(&mut buf).await.is_ok() {}
        });

        // When this participant's microphone track arrives, forward every RTP
        // packet to the other participants (and, later, the recorder).
        let weak: Weak<Call> = Arc::downgrade(&call);
        let me = principal_id.to_string();
        pc.on_track(Box::new(move |track: Arc<TrackRemote>, _, _| {
            let weak = weak.clone();
            let me = me.clone();
            Box::pin(async move {
                tokio::spawn(async move {
                    while let Ok((pkt, _)) = track.read_rtp().await {
                        let Some(call) = weak.upgrade() else { break };
                        for st in call.other_send_tracks(&me) {
                            // SAFETY of audio quality: same Opus payload, just relayed.
                            let _ = st.write_rtp(&pkt).await;
                        }
                        // Phase 5 tap: mux the same Opus into this speaker's .ogg.
                        call.recorder.write(&me, &pkt);
                    }
                });
            })
        }));

        // Trickle the server's ICE candidates to this participant.
        let sig = self.signal.clone();
        let room = room_id.to_string();
        let target = principal_id.to_string();
        let call_id = call.id.clone();
        pc.on_ice_candidate(Box::new(move |cand| {
            let sig = sig.clone();
            let room = room.clone();
            let target = target.clone();
            let call_id = call_id.clone();
            Box::pin(async move {
                let Some(cand) = cand else { return };
                let Ok(init) = cand.to_json() else { return };
                let Ok(candidate) = serde_json::to_string(&init) else {
                    return;
                };
                let _ = sig.send(Signal {
                    room_id: room,
                    target: Some(target),
                    exclude: None,
                    frame: Outbound::CallIce { call_id, candidate },
                });
            })
        }));

        // Offer first, then register the participant so forwarding can find it.
        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;

        let participant = Arc::new(Participant {
            id: principal_id.to_string(),
            name: principal_name.to_string(),
            pc,
            send_track,
        });
        call.participants
            .lock()
            .unwrap()
            .insert(principal_id.to_string(), participant);

        // Ring the room (others) and broadcast the updated roster.
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

        Ok((call.id.clone(), offer.sdp))
    }

    /// Apply a participant's SDP answer to the server's offer.
    pub async fn answer(&self, call_id: &str, principal_id: &str, sdp: String) -> Result<()> {
        let Some(call) = self.find(call_id) else {
            debug!(call_id, "answer for unknown call");
            return Ok(());
        };
        let pc = {
            let map = call.participants.lock().unwrap();
            map.get(principal_id).map(|p| p.pc.clone())
        };
        if let Some(pc) = pc {
            pc.set_remote_description(RTCSessionDescription::answer(sdp)?)
                .await?;
        }
        Ok(())
    }

    /// Add a participant's trickled ICE candidate.
    pub async fn ice(&self, call_id: &str, principal_id: &str, candidate: String) -> Result<()> {
        let Some(call) = self.find(call_id) else {
            return Ok(());
        };
        let pc = {
            let map = call.participants.lock().unwrap();
            map.get(principal_id).map(|p| p.pc.clone())
        };
        if let Some(pc) = pc {
            let init: RTCIceCandidateInit = serde_json::from_str(&candidate)?;
            pc.add_ice_candidate(init).await?;
        }
        Ok(())
    }

    /// A participant leaves. Closes their PeerConnection; when the call empties
    /// it is dropped (finalizing any Phase 5 recording) and the room told.
    pub async fn leave(&self, call_id: &str, principal_id: &str) {
        let Some(call) = self.find(call_id) else {
            return;
        };
        let (participant, now_empty) = {
            let mut map = call.participants.lock().unwrap();
            let p = map.remove(principal_id);
            (p, map.is_empty())
        };
        if let Some(p) = participant {
            let _ = p.pc.close().await;
        }
        if now_empty {
            self.calls.lock().unwrap().remove(&call.room_id);
            // Finalize the recording before marking the call ended; only point
            // `recording_id` at it if at least one track was captured.
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
