//! HLS network client (Phase 2 of HLS playback).
//!
//! Fetches HLS master/media playlists and media segments over HTTP, resolves relative
//! segment URIs against the playlist URL, demultiplexes MPEG-TS segments into ADTS AAC
//! frames, and decodes those frames into PCM samples via symphonia. The resulting PCM is
//! fed into a rodio [`HlsSource`] that the player appends to the mixer.

use std::io::Cursor;
use std::num::NonZero;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use reqwest::Client;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::probe::Hint;

use crate::audio::hls::playlist::{MasterPlaylist, MediaPlaylist, parse_master, parse_media};

/// Resolves a relative playlist/segment URI against a base HLS URL.
///
/// Handles absolute URLs (`http://…`), root-relative paths (`/a/b`), and relative paths
/// (`seg.ts`) the same way an HLS client resolves them.
pub fn resolve_uri(base: &str, uri: &str) -> String {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return uri.to_string();
    }
    // Strip the path after the last '/' of the base to get the directory.
    let dir_end = base.rfind('/').map(|i| i + 1).unwrap_or(base.len());
    let dir = &base[..dir_end];
    if uri.starts_with('/') {
        // Root-relative: take scheme+host from base.
        let scheme_end = base.find("//").map(|i| i + 2).unwrap_or(0);
        let host_end = base[scheme_end..]
            .find('/')
            .map(|i| scheme_end + i)
            .unwrap_or(base.len());
        format!("{}{}", &base[..host_end], uri)
    } else {
        format!("{}{}", dir, uri)
    }
}

/// An HLS client that fetches playlists and segments from a base URL.
pub struct HlsClient {
    client: Client,
    base_url: String,
}

impl HlsClient {
    pub fn new(client: Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    /// Fetches the master playlist at the base URL.
    pub async fn fetch_master(&self) -> Result<MasterPlaylist> {
        let text = self.fetch_text(&self.base_url).await?;
        parse_master(&text)
    }

    /// Fetches a media playlist at a (possibly relative) URI.
    pub async fn fetch_media(&self, uri: &str) -> Result<MediaPlaylist> {
        let url = resolve_uri(&self.base_url, uri);
        let text = self.fetch_text(&url).await?;
        parse_media(&text)
    }

    /// Fetches a media segment at a (possibly relative) URI as raw bytes.
    pub async fn fetch_segment(&self, uri: &str) -> Result<Vec<u8>> {
        let url = resolve_uri(&self.base_url, uri);
        self.fetch_bytes(&url).await
    }

    async fn fetch_text(&self, url: &str) -> Result<String> {
        let resp = self.client.get(url).send().await?;
        resp.text().await.map_err(Into::into)
    }

    async fn fetch_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.client.get(url).send().await?;
        resp.bytes().await.map(|b| b.to_vec()).map_err(Into::into)
    }
}

/// Demultiplexes a single, self-contained media segment into ADTS AAC frames.
///
/// Handles two segment container formats:
/// - **Shoutcast magic packet** (`0x28b5ff…`): the raw AAC follows the magic + header.
/// - **MPEG-TS** (188-byte packets): PAT → PMT → audio PES → ADTS frames.
///
/// Live playback uses [`TsDemuxer`], which carries a partial ADTS frame across segment
/// boundaries; this one-shot form is the entry point when a segment is handled alone.
#[allow(dead_code)]
pub fn demux_ts(segment: &[u8]) -> Vec<u8> {
    TsDemuxer::new().feed(segment)
}

/// Splits a TS segment into (pid, payload) pairs, dropping null packets (PID 0x1f).
///
/// Shoutcast HLS ships *non-standard* packets: the adaptation field control and the
/// continuity counter share byte 3 (`afc = byte3 >> 4`, `cc = byte3 & 0xf`), and
/// `payload_unit_start_indicator` does not mark PES starts. This is exactly the layout
/// FFMPEG's `mpegts.c` parses (`afc = (packet[3] >> 4) & 3`), and it is why
/// `oxideav-mpegts`'s `TsPacket::parse` rejects the stream (it returns an empty payload
/// when `afc & 1 == 0`), so we walk packets here.
fn split(segment: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    for packet in segment.chunks(188) {
        if packet.len() < 4 || packet[0] != 0x47 {
            continue;
        }
        // PID = the 13 low bits of bytes 1-2 (ffmpeg: `AV_RB16(packet + 1) & 0x1fff`).
        let pid = (((packet[1] as u16) << 8) | packet[2] as u16) & 0x1fff;
        let afc = (packet[3] >> 4) & 3;
        if afc == 0 {
            continue; // reserved value
        }
        // The adaptation field begins at byte 4 with its length byte, so a field of
        // length `n` occupies bytes 4..=4+n and the payload starts at 5 + n. ffmpeg
        // does `p = packet + 4; if (has_adaptation) p += p[0] + 1;`.
        let mut start = 4;
        if afc & 2 != 0 {
            start += packet[4] as usize + 1;
        }
        // afc & 1 is the payload flag: afc == 2 means adaptation field only.
        if afc & 1 == 0 || start >= packet.len() {
            continue;
        }
        let payload = &packet[start..];
        if payload.is_empty() || pid == 0x1f {
            continue;
        }
        out.push((pid, payload));
    }
    out
}

/// Resolves the audio elementary-stream PID by walking the PAT, then the PMT.
fn find_audio_pid(packets: &[(u16, &[u8])]) -> Option<u16> {
    // PAT: pointer_field, then table_id 0x00, section_length, program entries.
    // Section header = table_id(1) + section_length(2) + ts_id(2) + version(1)
    // + section_number(1) + last_section_number(1) = 8 bytes, so program entries
    // begin at section[8].
    let mut pmt_pid = None;
    for (pid, payload) in packets {
        if *pid != 0 {
            continue;
        }
        let pointer = payload[0] as usize;
        let section = payload.get(pointer + 1..)?;
        if section.len() >= 12 && section[0] == 0x00 {
            let section_len = ((section[1] & 0x0f) as u16) << 8 | section[2] as u16;
            let end = (3 + section_len as usize).min(section.len());
            let mut i = 8;
            while i + 4 <= end {
                let prog = (section[i] as u16) << 8 | section[i + 1] as u16;
                let pmt = ((section[i + 2] & 0x1f) as u16) << 8 | section[i + 3] as u16;
                if prog != 0 {
                    pmt_pid = Some(pmt);
                    break;
                }
                i += 4;
            }
        }
    }
    let pmt_pid = pmt_pid?;
    // PMT: pointer_field, table_id 0x02, program_number(2), version(1),
    // section_number(1), last_section_number(1), pcr_pid(2), then
    // program_info_length(2) at section[10]; stream entries begin at
    // section[12 + program_info_length].
    for (pid, payload) in packets {
        if *pid != pmt_pid {
            continue;
        }
        let pointer = payload[0] as usize;
        let section = payload.get(pointer + 1..)?;
        if section.len() >= 12 && section[0] == 0x02 {
            let section_len = ((section[1] & 0x0f) as u16) << 8 | section[2] as u16;
            let end = (3 + section_len as usize).min(section.len());
            let pil = ((section[10] & 0x0f) as u16) << 8 | section[11] as u16;
            let mut i = 12 + pil as usize;
            while i + 5 <= end {
                let stream_type = section[i];
                let es_pid = ((section[i + 1] & 0x1f) as u16) << 8 | section[i + 2] as u16;
                let es_info_len = ((section[i + 3] & 0x0f) as u16) << 8 | section[i + 4] as u16;
                if stream_type == 0x0f {
                    return Some(es_pid);
                }
                i += 5 + es_info_len as usize;
            }
        }
    }
    None
}

/// Total ADTS frame length (header + raw data) from a 7-byte header.
///
/// Bit layout matches symphonia's `AdtsReader` and ffmpeg's `adtsenc.c`: the header is
/// sync(12), id(1), layer(2), protection_absent(1), profile(2), sample_rate(4),
/// private(1), channels(3), 4 reserved bits, then frame_length(13). So `frame_length`
/// spans byte3 bits 1-0, all of byte4, and byte5 bits 7-5.
fn adts_frame_len(hdr: &[u8]) -> Option<usize> {
    if hdr.len() < 7 || hdr[0] != 0xff || (hdr[1] & 0xf6) != 0xf0 {
        return None;
    }
    let fl =
        ((hdr[3] & 0x03) as usize) << 11 | (hdr[4] as usize) << 3 | ((hdr[5] & 0xe0) as usize) >> 5;
    if fl < 7 {
        return None;
    }
    Some(fl)
}

/// Extracts complete ADTS frames from `buf`, returning them and the number of bytes
/// consumed.
///
/// A trailing partial frame is left unconsumed so the caller can carry it into the next
/// segment. PES headers, Shoutcast prefixes, and PES padding are skipped by scanning for
/// the ADTS sync pattern.
fn extract_adts(buf: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        match adts_frame_len(&buf[i..]) {
            Some(fl) if i + fl <= buf.len() => {
                out.extend_from_slice(&buf[i..i + fl]);
                i += fl;
            }
            Some(_) => break, // partial frame at the tail: carry it over
            None => i += 1,   // resync
        }
    }
    (out, i)
}

/// A stateful MPEG-TS demultiplexer for live HLS.
///
/// Live segments are 188-byte aligned, so an ADTS frame can straddle a segment boundary.
/// `feed` keeps the trailing partial frame and the resolved audio PID between segments so
/// no audio is lost across segments.
#[derive(Default)]
pub struct TsDemuxer {
    audio_pid: Option<u16>,
    carry: Vec<u8>,
}

impl TsDemuxer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one media segment and returns the complete ADTS frames produced so far.
    pub fn feed(&mut self, segment: &[u8]) -> Vec<u8> {
        if segment.len() >= 6 && segment[0] == 0x28 && segment[1] == 0xb5 && segment[2] == 0xff {
            // Shoutcast magic packet: raw AAC follows the magic + header.
            let header_len = ((segment[4] as usize) << 8) | segment[5] as usize;
            let start = 6 + header_len;
            if start < segment.len() {
                self.carry.extend_from_slice(&segment[start..]);
            }
        } else {
            let packets = split(segment);
            if self.audio_pid.is_none() {
                self.audio_pid = find_audio_pid(&packets);
            }
            if let Some(audio_pid) = self.audio_pid {
                for (pid, payload) in &packets {
                    if *pid == audio_pid {
                        self.carry.extend_from_slice(payload);
                    }
                }
            }
        }
        let (out, consumed) = extract_adts(&self.carry);
        self.carry = self.carry[consumed..].to_vec();
        out
    }

    /// Drops any buffered partial frame and the cached PID, e.g. on a discontinuity.
    pub fn reset(&mut self) {
        self.carry.clear();
        self.audio_pid = None;
    }
}

/// Decodes ADTS AAC frames into interleaved f32 PCM samples.
///
/// Returns `(samples, channels, sample_rate)`. Channels/sample_rate are taken from the
/// decoded signal spec (typically 2 / 44100 for AAC-LC).
pub fn decode_adts(adts: &[u8]) -> Result<(Vec<f32>, NonZero<u16>, NonZero<u32>)> {
    if adts.is_empty() {
        return Err(anyhow!("no ADTS frames to decode"));
    }
    let mut hint = Hint::new();
    hint.with_extension("aac");
    let mss = MediaSourceStream::new(Box::new(Cursor::new(adts.to_vec())), Default::default());
    let probe = symphonia::default::get_probe();
    let mut format = probe
        .format(&hint, mss, &Default::default(), &Default::default())?
        .format;
    let codecs = symphonia::default::get_codecs();
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track in ADTS stream"))?;
    let params = &track.codec_params;
    let mut decoder = codecs.make(params, &Default::default())?;

    let mut samples = Vec::new();
    let mut channels = NonZero::new(2u16).unwrap();
    let mut sample_rate = NonZero::new(44100u32).unwrap();
    while let Ok(packet) = format.next_packet() {
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(_) => break,
        };
        let spec = buf.spec();
        channels = NonZero::new(spec.channels.count() as u16).unwrap();
        sample_rate = NonZero::new(spec.rate).unwrap();
        let frames = buf.frames() as u64;
        let mut sb = SampleBuffer::<f32>::new(frames, *spec);
        sb.copy_interleaved_ref(buf);
        samples.extend_from_slice(sb.samples());
    }

    Ok((samples, channels, sample_rate))
}

/// A chunk of decoded audio (or a flush signal) sent from the HLS worker to the player.
///
/// `Samples` carries a decoded PCM chunk; `Flush` tells the [`HlsSource`] to drop its
/// current buffer so playback resumes cleanly after a tier switch / discontinuity.
pub enum HlsChunk {
    Samples {
        samples: Vec<f32>,
        channels: NonZero<u16>,
        sample_rate: NonZero<u32>,
    },
    Flush,
}

/// Live HLS playback status, shared between the HLS worker and the UI.
#[derive(Debug, Clone, Default)]
pub struct HlsStatus {
    /// Human-readable name of the active tier (e.g. `"lofi"`, `"hifi"`).
    pub tier_name: String,
    /// Estimated download bandwidth in bits per second.
    pub estimated_bitrate: u64,
    /// Whether an HLS stream is currently active.
    pub active: bool,
}

/// Maps a variant bandwidth (bits per second) to a tier name.
pub fn tier_name(bandwidth: u64) -> &'static str {
    if bandwidth <= 60_000 {
        "lofi"
    } else if bandwidth <= 120_000 {
        "midfi"
    } else {
        "hifi"
    }
}

/// A rodio [`Source`] that yields PCM samples received from an HLS background thread.
///
/// The background thread fetches segments, demuxes, decodes, and sends [`HlsChunk`]s over
/// the channel. Channels/sample_rate are updated on the first chunk; the source is created
/// with a 2/44100 placeholder. A [`HlsChunk::Flush`] drops the current buffer so playback
/// resumes cleanly after a tier switch.
pub struct HlsSource {
    rx: mpsc::Receiver<HlsChunk>,
    current: Vec<f32>,
    idx: usize,
    channels: NonZero<u16>,
    sample_rate: NonZero<u32>,
}

impl HlsSource {
    pub fn new(rx: mpsc::Receiver<HlsChunk>) -> Self {
        Self {
            rx,
            current: Vec::new(),
            idx: 0,
            channels: NonZero::new(2u16).unwrap(),
            sample_rate: NonZero::new(44100u32).unwrap(),
        }
    }
}

impl rodio::Source for HlsSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.current.len() - self.idx)
    }
    fn channels(&self) -> NonZero<u16> {
        self.channels
    }
    fn sample_rate(&self) -> NonZero<u32> {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
    fn try_seek(&mut self, _pos: Duration) -> Result<(), rodio::source::SeekError> {
        Err(rodio::source::SeekError::NotSupported {
            underlying_source: "HLS live stream",
        })
    }
}

impl Iterator for HlsSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.idx < self.current.len() {
            let s = self.current[self.idx];
            self.idx += 1;
            Some(s)
        } else {
            // Pull chunks until we get samples (skipping flushes).
            loop {
                match self.rx.recv() {
                    Ok(HlsChunk::Samples {
                        samples,
                        channels,
                        sample_rate,
                    }) => {
                        self.current = samples;
                        self.channels = channels;
                        self.sample_rate = sample_rate;
                        self.idx = 1;
                        return self.current.first().copied();
                    }
                    Ok(HlsChunk::Flush) => {
                        // Drop the current buffer; keep pulling for the next samples.
                        self.current.clear();
                        self.idx = 0;
                    }
                    Err(_) => return None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real MPEG-TS segment fetched from the Neuro Karaoke hifi tier.
    const SEG: &[u8] = include_bytes!("../../../raw_json/hls/current.ts");

    /// Demuxed ADTS must match ffmpeg's reference extraction byte-for-byte, and symphonia
    /// must decode every frame. Reference:
    /// `ffmpeg -i raw_json/hls/current.ts -c:a -f adts /tmp/ff.adts` → 97111 bytes,
    /// 172 AAC-LC frames at 44100 Hz stereo.
    #[test]
    fn demuxed_adts_matches_ffmpeg_reference_and_decodes() {
        let adts = demux_ts(SEG);
        assert_eq!(
            adts.len(),
            97111,
            "demuxed ADTS length differs from the ffmpeg reference"
        );
        assert_eq!(
            &adts[..7],
            [0xff, 0xf1, 0x50, 0x80, 0x45, 0xbf, 0xfc],
            "first ADTS header differs from the ffmpeg reference"
        );

        let mut hint = Hint::new();
        hint.with_extension("aac");
        let mss = MediaSourceStream::new(Box::new(Cursor::new(adts.to_vec())), Default::default());
        let probe = symphonia::default::get_probe();
        let mut f = probe
            .format(&hint, mss, &Default::default(), &Default::default())
            .expect("symphonia must accept the demuxed ADTS stream")
            .format;
        let params = f.tracks().first().unwrap().codec_params.clone();
        let codecs = symphonia::default::get_codecs();
        let mut dec = codecs.make(&params, &Default::default()).unwrap();
        let mut total = 0usize;
        let mut n = 0usize;
        let mut first_err = None;
        while let Ok(packet) = f.next_packet() {
            n += 1;
            match dec.decode(&packet) {
                Ok(buf) => total += buf.frames(),
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                    break;
                }
            }
        }
        assert!(first_err.is_none(), "symphonia failed to decode a frame");
        assert_eq!(n, 172, "expected 172 ADTS packets");
        assert_eq!(total, 172 * 1024, "expected 1024 samples per frame");
        assert_eq!(params.sample_rate, Some(44100));
        assert_eq!(params.channels.map(|c| c.count()), Some(2));
    }

    /// The stateful demuxer must produce the same bytes as the one-shot demuxer.
    #[test]
    fn ts_demuxer_matches_one_shot_demux() {
        let mut d = TsDemuxer::new();
        let streamed = d.feed(SEG);
        assert_eq!(streamed, demux_ts(SEG));
    }

    /// Feeding the segment in two halves must yield the same ADTS as feeding it whole:
    /// the trailing partial ADTS frame is carried across the boundary.
    #[test]
    fn ts_demuxer_carries_partial_frame_across_segments() {
        let cut = 188 * 300;
        let mut d = TsDemuxer::new();
        let mut streamed = d.feed(&SEG[..cut]);
        streamed.extend(d.feed(&SEG[cut..]));
        assert_eq!(streamed, demux_ts(SEG));
    }

    /// Live HLS delivers 188-byte-aligned segments, so ADTS frames straddle boundaries.
    /// Feeding the fixture as a sequence of such "segments" must reproduce the whole-segment
    /// ADTS exactly — the continuity property ABR tier switching relies on.
    #[test]
    fn ts_demuxer_survives_live_segment_split() {
        assert_eq!(SEG.len() % 188, 0, "fixture must be 188-byte aligned");
        let mut d = TsDemuxer::new();
        let mut streamed = Vec::new();
        for chunk in SEG.chunks(188 * 100) {
            streamed.extend(d.feed(chunk));
        }
        assert_eq!(streamed, demux_ts(SEG));
    }

    /// `reset` (tier switch / discontinuity) must leave no state behind: feeding the same
    /// segment after a reset yields the same ADTS as a cold demuxer.
    #[test]
    fn ts_demuxer_reset_leaves_no_state() {
        let mut d = TsDemuxer::new();
        let first = d.feed(SEG);
        d.reset();
        let second = d.feed(SEG);
        assert_eq!(second, first);
        assert_eq!(second, demux_ts(SEG));
    }

    /// Decoding ADTS produced by live-style split feeding must yield identical PCM: no
    /// samples are lost at segment boundaries.
    #[test]
    fn live_split_feeding_decodes_to_identical_pcm() {
        let mut d = TsDemuxer::new();
        let mut adts = Vec::new();
        for chunk in SEG.chunks(188 * 100) {
            adts.extend(d.feed(chunk));
        }
        let (whole, ch_w, sr_w) = decode_adts(&demux_ts(SEG)).expect("whole-segment decode failed");
        let (split, ch_s, sr_s) = decode_adts(&adts).expect("split-feed decode failed");
        assert_eq!(split, whole, "PCM differs when fed as live segments");
        assert_eq!((ch_s.get(), sr_s.get()), (ch_w.get(), sr_w.get()));
    }

    /// A segment carrying no PAT/PMT must yield no ADTS and must not panic.
    #[test]
    fn segment_without_pat_pmt_yields_no_adts() {
        // Null packets (PID 0x1f), afc = 1 (payload only), payload all 0xFF.
        let seg: Vec<u8> = (0..188 * 4)
            .map(|i| match i % 188 {
                0 => 0x47,
                1 => 0x00,
                2 => 0x1f,
                3 => 0x10,
                _ => 0xff,
            })
            .collect();
        let mut d = TsDemuxer::new();
        assert_eq!(d.feed(&seg), Vec::<u8>::new());
        assert_eq!(demux_ts(&seg), Vec::<u8>::new());
    }

    #[test]
    fn demuxes_real_ts_into_adts() {
        let adts = demux_ts(SEG);
        assert!(
            !adts.is_empty(),
            "no ADTS frames extracted from real TS segment"
        );
        // ADTS frames start with the 0xFF 0xFx sync pattern.
        assert_eq!(adts[0], 0xff);
        assert_eq!(adts[1] & 0xf6, 0xf0);
    }

    #[test]
    fn decodes_real_ts_segment_full() {
        let adts = demux_ts(SEG);
        let (samples, ch, sr) = decode_adts(&adts).expect("decode failed");
        // A ~4 s stereo AAC segment at 44100 Hz ≈ 352,800 samples. The decoder must
        // decode every packet, not skip every other one (which yields ~half).
        let expected = 4.0 * sr.get() as f64 * ch.get() as f64;
        let got = samples.len() as f64;
        assert!(
            got > expected * 0.9,
            "decoded {got} samples, expected ~{expected} (decoder is dropping packets)"
        );
    }
}
