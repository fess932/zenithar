//! Server-side call recording (Phase 5).
//!
//! The server already terminates DTLS/SRTP and forwards every participant's
//! decrypted Opus RTP (see [`crate::calls`]). Recording is therefore a *tap* on
//! that forward loop: we mux the same Opus packets straight into an Ogg/Opus
//! file — no decode/re-encode, so it's cheap and lossless.
//!
//! One file per participant, named `<call_id>.<participant_id>.ogg` under the
//! recordings dir. (Per-track, not mixed: mixing would require decoding +
//! re-encoding, which defeats the point of tapping the forwarded stream.)

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use rtc::media::io::ogg_writer::OggWriter;
use rtc::media::io::Writer;
use rtc::rtp::packet::Packet;
use tracing::{debug, warn};

// Opus in WebRTC is always 48 kHz; we tag the Ogg stream as stereo to match the
// forwarded track (players downmix mono payloads fine).
const OPUS_SAMPLE_RATE: u32 = 48000;
const OPUS_CHANNELS: u8 = 2;

/// Per-call recorder. A writer is opened lazily on a participant's first packet
/// and finalized when the call ends.
pub struct Recorder {
    call_id: String,
    dir: PathBuf,
    writers: Mutex<HashMap<String, OggWriter<BufWriter<File>>>>,
}

impl Recorder {
    pub fn new(dir: PathBuf, call_id: &str) -> Self {
        Self {
            call_id: call_id.to_string(),
            dir,
            writers: Mutex::new(HashMap::new()),
        }
    }

    fn open(&self, participant_id: &str) -> Result<OggWriter<BufWriter<File>>> {
        let path = self
            .dir
            .join(format!("{}.{participant_id}.ogg", self.call_id));
        let file = File::create(&path)?;
        Ok(OggWriter::new(
            BufWriter::new(file),
            OPUS_SAMPLE_RATE,
            OPUS_CHANNELS,
        )?)
    }

    /// Append one forwarded Opus RTP packet to `participant_id`'s track. Best
    /// effort: any error is logged and dropped so it never disrupts the call.
    pub fn write(&self, participant_id: &str, pkt: &Packet) {
        let mut writers = self.writers.lock().unwrap();
        if !writers.contains_key(participant_id) {
            match self.open(participant_id) {
                Ok(w) => {
                    writers.insert(participant_id.to_string(), w);
                }
                Err(e) => {
                    warn!(error = %e, participant = participant_id, "could not open recording");
                    return;
                }
            }
        }
        if let Some(w) = writers.get_mut(participant_id) {
            if let Err(e) = w.write_rtp(pkt) {
                debug!(error = %e, "recording write failed");
            }
        }
    }

    /// Close every track's file. Returns whether anything was recorded (so the
    /// caller can set `calls.recording_id` only when a recording actually exists).
    pub fn finalize(&self) -> bool {
        let mut writers = self.writers.lock().unwrap();
        let recorded = !writers.is_empty();
        for (_, mut w) in writers.drain() {
            if let Err(e) = w.close() {
                warn!(error = %e, "recording close failed");
            }
        }
        recorded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtc::rtp::header::Header;
    use rtc::rtp::packet::Packet;

    fn opus_pkt(timestamp: u32) -> Packet {
        Packet {
            header: Header {
                timestamp,
                ..Default::default()
            },
            // Any non-empty payload; the Opus depacketizer passes it through.
            payload: vec![0xf8u8, 0xff, 0xfe, 0x01, 0x02, 0x03].into(),
        }
    }

    #[test]
    fn writes_an_ogg_per_participant_and_reports_recorded() {
        let dir = std::env::temp_dir().join(format!(
            "zenithar-rec-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let rec = Recorder::new(dir.clone(), "call01");
        // Two participants, a couple of packets each (increasing timestamps).
        rec.write("alice", &opus_pkt(960));
        rec.write("alice", &opus_pkt(1920));
        rec.write("bob", &opus_pkt(960));
        assert!(rec.finalize(), "should report a recording exists");

        for who in ["alice", "bob"] {
            let path = dir.join(format!("call01.{who}.ogg"));
            let bytes = std::fs::read(&path).unwrap();
            assert!(bytes.len() > 4, "{who}.ogg should have content");
            assert_eq!(&bytes[0..4], b"OggS", "{who}.ogg must be an Ogg stream");
        }

        // An untouched recorder records nothing.
        let empty = Recorder::new(dir.clone(), "call02");
        assert!(!empty.finalize(), "no packets → no recording");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
