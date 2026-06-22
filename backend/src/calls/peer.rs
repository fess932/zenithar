//! Sans-IO (`rtc` crate) call media path — replacing the async `webrtc`-crate
//! path. The server is the WebRTC peer in the middle: it terminates DTLS/SRTP and
//! forwards Opus RTP between participants. Because `rtc` is sans-I/O (we drive
//! `poll_*`/`handle_*` against our own socket), the forwarding logic is unit
//! testable without real signaling or a browser — see the test below.
//!
//! This module is being built up incrementally alongside the legacy path.
#![allow(dead_code)]

use anyhow::Result;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::media_engine::{MediaEngine, MIME_TYPE_OPUS};
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
    RTCRtpHeaderExtensionCapability, RtpCodecKind,
};

/// Opus is the only codec we negotiate (voice-only calls).
pub(crate) const OPUS_PAYLOAD_TYPE: u8 = 111;
pub(crate) const OPUS_CLOCK_RATE: u32 = 48_000;
pub(crate) const OPUS_CHANNELS: u16 = 2;

/// RFC 8843 media-identification header extension. With BUNDLE (all tracks on one
/// transport) the browser demultiplexes inbound RTP onto the right `<audio>` track
/// by this MID; we must register it AND stamp it on every forwarded packet, or
/// multi-track (group) calls silently fail to route (1:1 works without it).
pub(crate) const SDES_MID_URI: &str = "urn:ietf:params:rtp-hdrext:sdes:mid";

/// The Opus codec parameters used on every call.
pub(crate) fn opus_codec() -> RTCRtpCodecParameters {
    RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: OPUS_CLOCK_RATE,
            channels: OPUS_CHANNELS,
            sdp_fmtp_line: String::new(),
            rtcp_feedback: vec![],
        },
        payload_type: OPUS_PAYLOAD_TYPE,
    }
}

/// A media engine registered for Opus audio + the MID header extension (so the
/// offer advertises `extmap` for it and BUNDLE demux can work — see SDES_MID_URI).
pub(crate) fn audio_media_engine() -> Result<MediaEngine> {
    let mut me = MediaEngine::default();
    me.register_codec(opus_codec(), RtpCodecKind::Audio)?;
    me.register_header_extension(
        RTCRtpHeaderExtensionCapability {
            uri: SDES_MID_URI.to_owned(),
        },
        RtpCodecKind::Audio,
        None,
    )?;
    Ok(me)
}

/// Parse our own offer SDP for the negotiated MID header-extension id and each
/// audio m-line's MID, in m-line order. Track slot `i` (the i-th `add_track`)
/// maps to the i-th audio m-line, so `mids[i]` is the MID to stamp for slot `i`.
pub(crate) fn parse_mid_layout(sdp: &str) -> (Option<u8>, Vec<String>) {
    let mut ext_id = None;
    let mut mids = Vec::new();
    for line in sdp.lines() {
        let line = line.trim();
        if ext_id.is_none() {
            if let Some(rest) = line.strip_prefix("a=extmap:") {
                // "<id>[/<direction>] <uri>"
                if rest.contains(SDES_MID_URI) {
                    if let Some(tok) = rest.split_whitespace().next() {
                        let tok = tok.split('/').next().unwrap_or(tok);
                        ext_id = tok.parse::<u8>().ok();
                    }
                }
            }
        }
        if let Some(mid) = line.strip_prefix("a=mid:") {
            mids.push(mid.to_string());
        }
    }
    (ext_id, mids)
}

/// An outbound Opus audio track for the server to send one participant's audio
/// down to another, carrying a fixed SSRC.
pub(crate) fn opus_track(stream_id: &str, track_id: &str, ssrc: u32) -> MediaStreamTrack {
    let codings = vec![RTCRtpEncodingParameters {
        rtp_coding_parameters: RTCRtpCodingParameters {
            ssrc: Some(ssrc),
            ..Default::default()
        },
        codec: opus_codec().rtp_codec,
        ..Default::default()
    }];
    MediaStreamTrack::new(
        stream_id.to_owned(),
        track_id.to_owned(),
        track_id.to_owned(),
        RtpCodecKind::Audio,
        codings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Bytes, BytesMut};
    use rtc::peer_connection::configuration::setting_engine::SettingEngine;
    use rtc::peer_connection::configuration::RTCConfigurationBuilder;
    use rtc::peer_connection::event::RTCPeerConnectionEvent;
    use rtc::peer_connection::message::RTCMessage;
    use rtc::peer_connection::state::RTCPeerConnectionState;
    use rtc::peer_connection::transport::{
        CandidateConfig, CandidateHostConfig, RTCDtlsRole, RTCIceCandidate,
    };
    use rtc::peer_connection::RTCPeerConnectionBuilder;
    use rtc::rtp;
    use rtc::sansio::Protocol;
    use rtc::shared::{TaggedBytesMut, TransportContext, TransportProtocol};
    use std::time::{Duration, Instant};
    use tokio::net::UdpSocket;

    /// Two `rtc` peers, offerer (sender) and answerer (receiver), connect over
    /// loopback UDP and the offerer's Opus RTP reaches the answerer — entirely in
    /// process, no browser, no signaling server. This is the deterministic test
    /// the sans-IO design buys us (impossible with the async `webrtc` crate).
    #[tokio::test]
    async fn opus_rtp_flows_between_two_rtc_peers() -> Result<()> {
        let off_sock = UdpSocket::bind("127.0.0.1:0").await?;
        let ans_sock = UdpSocket::bind("127.0.0.1:0").await?;
        let off_addr = off_sock.local_addr()?;
        let ans_addr = ans_sock.local_addr()?;

        let build = |dtls_server: bool| -> Result<_> {
            let mut se = SettingEngine::default();
            if dtls_server {
                se.set_answering_dtls_role(RTCDtlsRole::Server)?;
            }
            let pc = RTCPeerConnectionBuilder::new()
                .with_configuration(RTCConfigurationBuilder::new().build())
                .with_setting_engine(se)
                .with_media_engine(audio_media_engine()?)
                .build()?;
            Ok(pc)
        };
        let mut offerer = build(false)?;
        let mut answerer = build(true)?;

        let host = |addr: std::net::SocketAddr| -> Result<RTCIceCandidate> {
            let cand = CandidateHostConfig {
                base_config: CandidateConfig {
                    network: "udp".to_owned(),
                    address: addr.ip().to_string(),
                    port: addr.port(),
                    component: 1,
                    ..Default::default()
                },
                ..Default::default()
            }
            .new_candidate_host()?;
            Ok(RTCIceCandidate::from(&cand))
        };
        // Candidates added before offer/answer ride along in the SDP.
        offerer.add_local_candidate(host(off_addr)?.to_json()?)?;
        answerer.add_local_candidate(host(ans_addr)?.to_json()?)?;

        let ssrc = 0x1234_5678;
        let sender_id = offerer.add_track(opus_track("zenithar", "voice", ssrc))?;

        let offer = offerer.create_offer(None)?;
        offerer.set_local_description(offer.clone())?;
        answerer.set_remote_description(offer)?;
        let answer = answerer.create_answer(None)?;
        answerer.set_local_description(answer.clone())?;
        offerer.set_remote_description(answer)?;

        let mut off_buf = vec![0u8; 2000];
        let mut ans_buf = vec![0u8; 2000];
        let mut off_connected = false;
        let mut ans_connected = false;
        let mut seq: u16 = 0;
        let mut received = 0usize;

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline && received < 5 {
            while let Some(msg) = Protocol::poll_write(&mut offerer) {
                let _ = off_sock
                    .send_to(&msg.message, msg.transport.peer_addr)
                    .await;
            }
            while let Some(msg) = Protocol::poll_write(&mut answerer) {
                let _ = ans_sock
                    .send_to(&msg.message, msg.transport.peer_addr)
                    .await;
            }

            while let Some(ev) = Protocol::poll_event(&mut offerer) {
                if let RTCPeerConnectionEvent::OnConnectionStateChangeEvent(s) = ev {
                    if s == RTCPeerConnectionState::Connected {
                        off_connected = true;
                    }
                }
            }
            while let Some(ev) = Protocol::poll_event(&mut answerer) {
                if let RTCPeerConnectionEvent::OnConnectionStateChangeEvent(s) = ev {
                    if s == RTCPeerConnectionState::Connected {
                        ans_connected = true;
                    }
                }
            }

            while Protocol::poll_read(&mut offerer).is_some() {} // drain (ignore)
            while let Some(m) = Protocol::poll_read(&mut answerer) {
                if let RTCMessage::RtpPacket(_, _) = m {
                    received += 1;
                }
            }

            if off_connected && ans_connected {
                if let Some(mut sender) = offerer.rtp_sender(sender_id) {
                    seq = seq.wrapping_add(1);
                    let pkt = rtp::packet::Packet {
                        header: rtp::header::Header {
                            version: 2,
                            payload_type: OPUS_PAYLOAD_TYPE,
                            sequence_number: seq,
                            timestamp: seq as u32 * 960,
                            ssrc,
                            ..Default::default()
                        },
                        payload: Bytes::from_static(&[0xAA; 40]),
                    };
                    let _ = sender.write_rtp(pkt);
                }
            }

            let eto = [
                Protocol::poll_timeout(&mut offerer),
                Protocol::poll_timeout(&mut answerer),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or_else(|| Instant::now() + Duration::from_millis(20));
            let delay = eto
                .checked_duration_since(Instant::now())
                .unwrap_or_default()
                .min(Duration::from_millis(20));

            let timer = tokio::time::sleep(delay);
            tokio::pin!(timer);
            tokio::select! {
                _ = &mut timer => {
                    offerer.handle_timeout(Instant::now())?;
                    answerer.handle_timeout(Instant::now())?;
                }
                r = off_sock.recv_from(&mut off_buf) => {
                    if let Ok((n, peer)) = r {
                        offerer.handle_read(tagged(&off_buf[..n], off_addr, peer))?;
                    }
                }
                r = ans_sock.recv_from(&mut ans_buf) => {
                    if let Ok((n, peer)) = r {
                        answerer.handle_read(tagged(&ans_buf[..n], ans_addr, peer))?;
                    }
                }
            }
        }

        assert!(off_connected && ans_connected, "peers should connect");
        assert!(received >= 1, "answerer should receive forwarded Opus RTP");
        Ok(())
    }

    /// The offer must advertise the MID extmap (so BUNDLE demux can work) and
    /// `parse_mid_layout` must recover the id + one MID per outbound track, in
    /// order. This is what lets group calls route audio; the live browser-side
    /// demux is verified on a real deploy.
    #[test]
    fn offer_advertises_mid_and_layout_parses() -> Result<()> {
        let mut pc = RTCPeerConnectionBuilder::new()
            .with_configuration(RTCConfigurationBuilder::new().build())
            .with_media_engine(audio_media_engine()?)
            .build()?;
        let tracks = 8;
        for i in 0..tracks {
            pc.add_track(opus_track(
                &format!("z-{i}"),
                &format!("voice-{i}"),
                1000 + i,
            ))?;
        }
        let offer = pc.create_offer(None)?;
        assert!(
            offer.sdp.contains(SDES_MID_URI),
            "offer must carry the MID extmap"
        );
        let (ext_id, mids) = parse_mid_layout(&offer.sdp);
        assert!(ext_id.is_some(), "MID extension id must be negotiated");
        assert_eq!(mids.len(), tracks as usize, "one MID per audio m-line");
        Ok(())
    }

    fn tagged(
        buf: &[u8],
        local: std::net::SocketAddr,
        peer: std::net::SocketAddr,
    ) -> TaggedBytesMut {
        TaggedBytesMut {
            now: Instant::now(),
            transport: TransportContext {
                local_addr: local,
                peer_addr: peer,
                ecn: None,
                transport_protocol: TransportProtocol::UDP,
            },
            message: BytesMut::from(buf),
        }
    }
}
