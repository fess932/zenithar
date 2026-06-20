//! Single-file call recording: a live mixdown of every participant into one
//! Ogg/Opus. Where [`crate::calls::recorder`] taps the forwarded Opus per track
//! (cheap, lossless, but N files), this decodes each participant's Opus to stereo
//! PCM and simply SUMS them onto a shared timeline placed by real arrival time,
//! then re-encodes once to stereo Opus at a modest bitrate (default 64 kbps,
//! tiny files). No panning — the streams are just merged. Best effort: any decode
//! error is dropped.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use audiopus::coder::{Decoder, Encoder};
use audiopus::{Application, Bitrate, Channels, SampleRate};
use bytes::Bytes;
use rtc::media::io::ogg_writer::OggWriter;
use rtc::media::io::Writer;
use rtc::rtp::header::Header;
use rtc::rtp::packet::Packet;
use tracing::{debug, warn};

const RATE: usize = 48_000; // matches Opus internal rate (no resample)
const FRAME: usize = 960; // 20 ms @ 48 kHz, per channel — one Opus frame
const MAX_FRAME: usize = 5_760; // 120 ms @ 48 kHz, per channel — largest frame
const DEFAULT_BITRATE: i32 = 64_000; // voice: ~transparent, tiny files

struct Inner {
    start: Instant,
    /// Interleaved stereo accumulator (L, R, L, R…); i32 so summed speakers can't
    /// clip before the final clamp.
    buf: Vec<i32>,
    /// One stereo Opus decoder per participant (decoder state is per-stream).
    decoders: HashMap<String, Decoder>,
}

/// Per-call live mixer.
pub struct Mixer {
    call_id: String,
    dir: PathBuf,
    inner: Mutex<Inner>,
}

impl Mixer {
    pub fn new(dir: PathBuf, call_id: &str) -> Self {
        Self {
            call_id: call_id.to_string(),
            dir,
            inner: Mutex::new(Inner {
                start: Instant::now(),
                buf: Vec::new(),
                decoders: HashMap::new(),
            }),
        }
    }

    /// Decode one participant's Opus packet (as stereo) and sum it into the call
    /// timeline at its real (wall-clock) arrival position.
    pub fn add(&self, participant: &str, opus: &[u8]) {
        if opus.is_empty() {
            return;
        }
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        let pos = inner.start.elapsed().as_millis() as usize * RATE / 1000;

        let dec = inner.decoders.entry(participant.to_string()).or_insert_with(|| {
            Decoder::new(SampleRate::Hz48000, Channels::Stereo).expect("opus stereo decoder")
        });
        // Interleaved L/R out; decode returns samples PER CHANNEL.
        let mut out = [0i16; MAX_FRAME * 2];
        let n = match dec.decode(Some(opus), &mut out[..], false) {
            Ok(n) => n,
            Err(e) => {
                debug!(error = %e, "mix opus decode failed");
                return;
            }
        };

        let end = (pos + n) * 2;
        if inner.buf.len() < end {
            inner.buf.resize(end, 0);
        }
        for i in 0..n {
            inner.buf[(pos + i) * 2] += out[2 * i] as i32;
            inner.buf[(pos + i) * 2 + 1] += out[2 * i + 1] as i32;
        }
    }

    /// Encode the mixed stereo timeline to `<call_id>.mix.ogg`. Returns whether a
    /// file was written (false if nothing was ever mixed).
    pub fn finalize(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.buf.is_empty() {
            return false;
        }
        let bitrate = std::env::var("ZENITHAR_RECORD_BITRATE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_BITRATE);
        let path = self.dir.join(format!("{}.mix.ogg", self.call_id));
        match write_opus(&path, &inner.buf, bitrate) {
            Ok(()) => true,
            Err(e) => {
                warn!(error = %e, "mix opus write failed");
                false
            }
        }
    }
}

/// Encode the interleaved-stereo i32 accumulator to Ogg/Opus at `bitrate`.
fn write_opus(path: &Path, stereo: &[i32], bitrate: i32) -> anyhow::Result<()> {
    let mut enc = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Voip)?;
    enc.set_bitrate(Bitrate::BitsPerSecond(bitrate))?;

    let mut writer = OggWriter::new(BufWriter::new(File::create(path)?), RATE as u32, 2)?;
    let mut pcm = [0i16; FRAME * 2]; // interleaved L/R for one 20 ms frame
    let mut packet_buf = [0u8; 4000];
    let mut ts: u32 = 0;
    let mut seq: u16 = 0;

    for chunk in stereo.chunks(FRAME * 2) {
        pcm.fill(0);
        for (i, &v) in chunk.iter().enumerate() {
            pcm[i] = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        let n = enc.encode(&pcm[..], &mut packet_buf[..])?;
        let packet = Packet {
            header: Header {
                payload_type: 111,
                sequence_number: seq,
                timestamp: ts,
                ..Default::default()
            },
            payload: Bytes::copy_from_slice(&packet_buf[..n]),
        };
        writer.write_rtp(&packet)?;
        ts = ts.wrapping_add(FRAME as u32); // Opus RTP clock is 48 kHz per channel
        seq = seq.wrapping_add(1);
    }
    writer.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a tone to Opus, mix two participants through the full decode + sum +
    /// re-encode path, and confirm an Ogg/Opus file comes out — exercises libopus
    /// (decode and encode) end to end.
    #[test]
    fn mixes_two_streams_into_one_ogg() {
        let enc = Encoder::new(SampleRate::Hz48000, Channels::Mono, Application::Voip).unwrap();
        let tone: Vec<i16> = (0..FRAME)
            .map(|i| ((i as f32 * 0.2).sin() * 6000.0) as i16)
            .collect();
        let mut packet = [0u8; 4000];
        let len = enc.encode(&tone[..], &mut packet[..]).unwrap();
        let opus = &packet[..len];

        let dir = std::env::temp_dir().join(format!(
            "zenithar-mix-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let mix = Mixer::new(dir.clone(), "call01");
        for _ in 0..5 {
            mix.add("alice", opus);
            mix.add("bob", opus);
        }
        assert!(mix.finalize(), "should write a mix file");

        let bytes = std::fs::read(dir.join("call01.mix.ogg")).unwrap();
        assert_eq!(&bytes[0..4], b"OggS", "mix must be an Ogg stream");
        assert!(bytes.len() > 4, "mix should carry audio");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
