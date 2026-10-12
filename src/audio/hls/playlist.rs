//! HLS playlist parsing (Phase 1 of HLS playback).
//!
//! Wraps `quick-m3u8` to parse two kinds of HLS playlists:
//! - **Master playlists** (`#EXT-X-STREAM-INF`): a list of variant streams, each with a
//!   bandwidth, codec string, optional resolution, and a URI pointing to a media playlist.
//! - **Media playlists** (`#EXTINF`): an ordered list of media segments, each with a duration
//!   and a URI, plus the playlist's target duration, media sequence, and discontinuity
//!   sequence.
//!
//! For audio-only karaoke radio we care about the audio codec (`mp4a.40.2` = AAC-LC) and the
//! segment URIs. The decoder is intentionally tolerant: unknown/unsupported tags are skipped
//! rather than treated as fatal, matching how HLS clients behave in the wild.

use anyhow::{Result, anyhow};
use quick_m3u8::config::ParsingOptions;
use quick_m3u8::tag::{KnownTag, hls};
use quick_m3u8::{HlsLine, Reader};

/// A single variant stream in an HLS master playlist (`#EXT-X-STREAM-INF`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// Peak/average bandwidth in bits per second (`BANDWIDTH`).
    pub bandwidth: u64,
    /// Codec string, e.g. `"mp4a.40.2"` (`CODECS`).
    pub codecs: Option<String>,
    /// Optional `width x height` (`RESOLUTION`).
    pub resolution: Option<(u64, u64)>,
    /// Optional audio group name (`AUDIO`).
    pub audio: Option<String>,
    /// URI of the media playlist for this variant.
    pub uri: String,
}

/// A parsed HLS master playlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MasterPlaylist {
    pub variants: Vec<Variant>,
}

impl MasterPlaylist {
    /// Returns true if this playlist contains at least one variant stream.
    pub fn is_master(&self) -> bool {
        !self.variants.is_empty()
    }

    /// Picks the variant with the lowest bandwidth — the audio-only friendly default.
    pub fn lowest_bandwidth(&self) -> Option<&Variant> {
        self.variants.iter().min_by_key(|v| v.bandwidth)
    }

    /// Picks the variant whose codec string contains the given codec (e.g. `"mp4a"`),
    /// preferring the lowest bandwidth among matches.
    pub fn best_audio_variant(&self) -> Option<&Variant> {
        self.variants
            .iter()
            .filter(|v| {
                v.codecs
                    .as_deref()
                    .map(|c| c.contains("mp4a"))
                    .unwrap_or(true)
            })
            .min_by_key(|v| v.bandwidth)
    }
}

/// A single media segment in an HLS media playlist (`#EXTINF` + URI).
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Absolute media sequence number of this segment.
    pub sequence: u64,
    /// Segment duration in seconds (`EXTINF`).
    pub duration: f64,
    /// URI of the segment (e.g. a `.ts` file).
    pub uri: String,
    /// Whether a `#EXT-X-DISCONTINUITY` marker precedes this segment.
    pub discontinuity: bool,
}

/// A parsed HLS media playlist.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaPlaylist {
    pub target_duration: u64,
    pub media_sequence: u64,
    pub discontinuity_sequence: u64,
    pub segments: Vec<Segment>,
    pub endlist: bool,
}

impl MediaPlaylist {
    /// The newest segment in the playlist (the last appended entry), which is what a live
    /// client wants to fetch next.
    pub fn latest_segment(&self) -> Option<&Segment> {
        self.segments.last()
    }

    /// The segment at a given absolute media sequence number, if present.
    pub fn segment_at(&self, sequence: u64) -> Option<&Segment> {
        self.segments.iter().find(|s| s.sequence == sequence)
    }
}

/// Parses an HLS master playlist string into a [`MasterPlaylist`].
pub fn parse_master(playlist: &str) -> Result<MasterPlaylist> {
    let mut reader = Reader::from_str(playlist, ParsingOptions::default());
    let mut variants: Vec<Variant> = Vec::new();
    // A `#EXT-X-STREAM-INF` tag is always followed by the variant URI on the next line.
    let mut pending: Option<Variant> = None;

    loop {
        let line = match reader.read_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => return Err(anyhow!("failed to parse HLS master playlist: {e}")),
        };
        match line {
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::StreamInf(tag))) => {
                pending = Some(Variant {
                    bandwidth: tag.bandwidth(),
                    codecs: tag.codecs().map(|c| c.to_string()),
                    resolution: tag.resolution().map(|r| (r.width, r.height)),
                    audio: tag.audio().map(|a| a.to_string()),
                    uri: String::new(),
                });
            }
            HlsLine::Uri(uri) => {
                if let Some(mut v) = pending.take() {
                    v.uri = uri.trim().to_string();
                    variants.push(v);
                }
            }
            _ => {}
        }
    }

    Ok(MasterPlaylist { variants })
}

/// Parses an HLS media playlist string into a [`MediaPlaylist`].
pub fn parse_media(playlist: &str) -> Result<MediaPlaylist> {
    let mut reader = Reader::from_str(playlist, ParsingOptions::default());
    let mut target_duration = 0u64;
    let mut media_sequence = 0u64;
    let mut discontinuity_sequence = 0u64;
    let mut segments: Vec<Segment> = Vec::new();
    let mut endlist = false;

    // Sequence counter: starts at `media_sequence` once known, incremented per segment.
    let mut next_sequence: Option<u64> = None;
    let mut pending_duration: Option<f64> = None;
    let mut pending_discontinuity = false;

    loop {
        let line = match reader.read_line() {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(e) => return Err(anyhow!("failed to parse HLS media playlist: {e}")),
        };
        match line {
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::Targetduration(tag))) => {
                target_duration = tag.target_duration();
            }
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::MediaSequence(tag))) => {
                media_sequence = tag.media_sequence();
                next_sequence = Some(media_sequence);
            }
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::DiscontinuitySequence(tag))) => {
                discontinuity_sequence = tag.discontinuity_sequence();
            }
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::Discontinuity(_))) => {
                pending_discontinuity = true;
            }
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::Inf(tag))) => {
                pending_duration = Some(tag.duration());
            }
            HlsLine::KnownTag(KnownTag::Hls(hls::Tag::Endlist(_))) => {
                endlist = true;
            }
            HlsLine::Uri(uri) => {
                let duration = pending_duration.take().unwrap_or(0.0);
                let sequence = next_sequence.unwrap_or(media_sequence);
                segments.push(Segment {
                    sequence,
                    duration,
                    uri: uri.trim().to_string(),
                    discontinuity: pending_discontinuity,
                });
                next_sequence = Some(sequence + 1);
                pending_discontinuity = false;
            }
            _ => {}
        }
    }

    Ok(MediaPlaylist {
        target_duration,
        media_sequence,
        discontinuity_sequence,
        segments,
        endlist,
    })
}

/// Resolves a relative playlist/segment URI against a base URL.
///
/// Handles absolute URIs (returned as-is), root-relative URIs (`/path`), and relative
/// URIs resolved against the base URL's directory.
pub fn resolve_uri(base: &str, uri: &str) -> String {
    if uri.is_empty() {
        return base.to_string();
    }
    // Already an absolute URL.
    if uri.starts_with("http://") || uri.starts_with("https://") {
        return uri.to_string();
    }
    // Strip the scheme + host, then handle root-relative vs. relative.
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = base[scheme_end..]
        .find('/')
        .map(|i| scheme_end + i)
        .unwrap_or(base.len());
    let host = &base[..host_end];

    if uri.starts_with('/') {
        format!("{host}{uri}")
    } else {
        // Resolve relative to the base URL's directory.
        let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or(host);
        format!("{dir}/{uri}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-STREAM-INF:BANDWIDTH=52800,CODECS="mp4a.40.2"
aac_lofi.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=105600,CODECS="mp4a.40.2"
aac_midfi.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=211200,CODECS="mp4a.40.2"
aac_hifi.m3u8
"#;

    const MEDIA: &str = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:4
#EXT-X-MEDIA-SEQUENCE:541647
#EXTINF:4.000,
segment-541647.ts
#EXTINF:4.000,
segment-541648.ts
#EXTINF:4.000,
segment-541649.ts
#EXT-X-ENDLIST
"#;

    #[test]
    fn parses_master_variants() {
        let master = parse_master(MASTER).unwrap();
        assert_eq!(master.variants.len(), 3);
        assert_eq!(master.variants[0].bandwidth, 52800);
        assert_eq!(master.variants[0].codecs.as_deref(), Some("mp4a.40.2"));
        assert_eq!(master.variants[0].uri, "aac_lofi.m3u8");
        assert_eq!(master.variants[2].bandwidth, 211200);
        assert!(master.is_master());
        assert_eq!(master.lowest_bandwidth().unwrap().uri, "aac_lofi.m3u8");
        assert_eq!(master.best_audio_variant().unwrap().uri, "aac_lofi.m3u8");
    }

    #[test]
    fn parses_media_segments() {
        let media = parse_media(MEDIA).unwrap();
        assert_eq!(media.target_duration, 4);
        assert_eq!(media.media_sequence, 541647);
        assert_eq!(media.segments.len(), 3);
        assert_eq!(media.segments[0].sequence, 541647);
        assert_eq!(media.segments[0].duration, 4.0);
        assert_eq!(media.segments[0].uri, "segment-541647.ts");
        assert_eq!(media.segments[2].sequence, 541649);
        assert!(media.endlist);
        assert_eq!(media.latest_segment().unwrap().uri, "segment-541649.ts");
        assert!(media.segment_at(541648).is_some());
    }

    #[test]
    fn resolves_relative_uris() {
        let base = "https://radio.example.com/hls/live.m3u8";
        assert_eq!(
            resolve_uri(base, "aac_lofi.m3u8"),
            "https://radio.example.com/hls/aac_lofi.m3u8"
        );
        assert_eq!(
            resolve_uri(base, "/hls/seg.ts"),
            "https://radio.example.com/hls/seg.ts"
        );
        assert_eq!(
            resolve_uri(base, "https://cdn.example.com/a.ts"),
            "https://cdn.example.com/a.ts"
        );
    }

    #[test]
    fn parses_real_sample_master() {
        let raw = include_str!("../../../raw_json/hls/live.m3u8");
        let master = parse_master(raw).unwrap();
        assert_eq!(master.variants.len(), 3);
        assert_eq!(master.lowest_bandwidth().unwrap().bandwidth, 52800);
    }
}
