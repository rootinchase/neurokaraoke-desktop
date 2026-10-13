use std::time::Duration;

use crate::api::LyricEntry;

/// One parsed, timestamped lyric line. `start` is the line's timestamp; `end`
/// is the next line's start (set by `parse_lrc`), or `u64::MAX` seconds for the
/// last line.
#[derive(Debug, Clone)]
pub struct LyricLine {
    pub start: Duration,
    pub end: Duration,
    pub text: String,
}

/// The result of parsing an LRC payload. `lines` are the timestamped lines
/// sorted by start time; `unsynced` holds lines that carried no timestamps.
#[derive(Debug, Clone, Default)]
pub struct LyricsParsed {
    pub lines: Vec<LyricLine>,
    pub unsynced: Vec<String>,
}

impl LyricsParsed {
    /// Index of the active line for `pos`, or `None` when playback is before
    /// the first timestamp.
    pub fn active_index(&self, pos: Duration) -> Option<usize> {
        match self.lines.binary_search_by(|l| l.start.cmp(&pos)) {
            Ok(i) => Some(i),
            Err(0) => None,
            Err(i) => Some(i - 1),
        }
    }
}

/// Parses LRC text into timestamped lines plus any unsynced lines.
///
/// Handles `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]` timestamps, multiple
/// timestamps on one line (`[00:01][00:30]text`), a global `[offset:±ms]`,
/// metadata tags (`[ti:]`, `[ar:]`, …, skipped), and lines with no timestamps.
#[allow(dead_code)]
pub fn parse_lrc(text: &str) -> LyricsParsed {
    let mut offset: i64 = 0;
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut unsynced: Vec<String> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Consume all leading `[..]` tags; collect timestamps, apply offset,
        // and skip metadata tags.
        let mut timestamps: Vec<i64> = Vec::new();
        let mut rest = line;
        while rest.starts_with('[') {
            if let Some(end) = rest.find(']') {
                let tag = &rest[1..end];
                if let Some(ms) = parse_timestamp(tag) {
                    timestamps.push(ms);
                } else if let Some(off) = parse_offset(tag) {
                    offset += off;
                }
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }

        let content = rest.trim();
        if timestamps.is_empty() {
            if !content.is_empty() {
                unsynced.push(content.to_string());
            }
        } else if !content.is_empty() {
            for ts in timestamps {
                timed.push((ts, content.to_string()));
            }
        }
    }

    timed.sort_by_key(|(ts, _)| *ts);

    let mut lines: Vec<LyricLine> = Vec::new();
    for (ts, content) in timed {
        let start_ms = (ts + offset).max(0);
        lines.push(LyricLine {
            start: Duration::from_millis(start_ms as u64),
            end: Duration::ZERO,
            text: content,
        });
    }

    for i in 0..lines.len() {
        lines[i].end = lines
            .get(i + 1)
            .map(|n| n.start)
            .unwrap_or(Duration::from_secs(u64::MAX));
    }

    LyricsParsed { lines, unsynced }
}

/// Parses the first-party `GET /api/songs/{id}/lyrics` JSON array into
/// timestamped lines. Each entry's `time` is a .NET `TimeSpan`
/// (`HH:mm:ss.fffffff`); lines are sorted by timestamp.
pub fn parse_time_entries(entries: &[LyricEntry]) -> LyricsParsed {
    let mut timed: Vec<(Duration, String)> = entries
        .iter()
        .filter_map(|e| parse_timespan(&e.time).map(|d| (d, e.text.clone())))
        .collect();
    timed.sort_by_key(|(d, _)| *d);

    let mut lines: Vec<LyricLine> = Vec::new();
    for (d, text) in timed {
        lines.push(LyricLine {
            start: d,
            end: Duration::ZERO,
            text,
        });
    }
    for i in 0..lines.len() {
        lines[i].end = lines
            .get(i + 1)
            .map(|n| n.start)
            .unwrap_or(Duration::from_secs(u64::MAX));
    }

    LyricsParsed {
        lines,
        unsynced: Vec::new(),
    }
}

/// Parses a .NET `TimeSpan` (`HH:mm:ss.fffffff`) into a `Duration`, truncating
/// the fractional part to milliseconds.
fn parse_timespan(s: &str) -> Option<Duration> {
    let (h, rest) = s.split_once(':')?;
    let (m, rest) = rest.split_once(':')?;
    let (sec, frac) = match rest.split_once('.') {
        Some(a) => a,
        None => (rest, ""),
    };
    let h = h.parse::<u64>().ok()?;
    let m = m.parse::<u64>().ok()?;
    let sec = sec.parse::<u64>().ok()?;
    let ms = match frac.len() {
        0 => 0,
        _ => {
            let f: String = frac.chars().take(3).collect();
            f.parse::<u64>().ok()?
        }
    };
    Some(Duration::from_millis((h * 3600 + m * 60 + sec) * 1000 + ms))
}

/// Parses a `[mm:ss]`, `[mm:ss.x]`, `[mm:ss.xx]`, or `[mm:ss.xxx]` timestamp
/// into milliseconds. Returns `None` for metadata tags (e.g. `ti:Title`).
#[allow(dead_code)]
fn parse_timestamp(tag: &str) -> Option<i64> {
    let (mm, ss) = tag.split_once(':')?;
    let mm = mm.parse::<i64>().ok()?;
    let (ss, frac) = match ss.split_once('.') {
        Some(a) => a,
        None => (ss, ""),
    };
    let ss = ss.parse::<i64>().ok()?;
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => {
            let f: String = frac.chars().take(3).collect();
            f.parse::<i64>().ok()?
        }
    };
    Some((mm * 60 + ss) * 1000 + frac_ms)
}

/// Parses an `[offset:±ms]` tag into a millisecond delta.
#[allow(dead_code)]
fn parse_offset(tag: &str) -> Option<i64> {
    let off = tag.strip_prefix("offset:")?;
    off.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_timestamps() {
        let p = parse_lrc("[00:12.00]line one\n[00:15.50]line two");
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.lines[0].start, Duration::from_millis(12000));
        assert_eq!(p.lines[1].start, Duration::from_millis(15500));
        assert_eq!(p.lines[0].end, Duration::from_millis(15500));
        assert_eq!(p.lines[0].text, "line one");
    }

    #[test]
    fn parses_multiple_timestamps_per_line() {
        let p = parse_lrc("[00:01][00:30]text");
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.lines[0].start, Duration::from_millis(1000));
        assert_eq!(p.lines[1].start, Duration::from_millis(30000));
        assert_eq!(p.lines[0].text, "text");
        assert_eq!(p.lines[1].text, "text");
    }

    #[test]
    fn applies_global_offset() {
        let p = parse_lrc("[offset:+1000]\n[00:01.00]x");
        assert_eq!(p.lines[0].start, Duration::from_millis(2000));
    }

    #[test]
    fn skips_metadata_tags() {
        let p = parse_lrc("[ti:Title]\n[ar:Artist]\n[00:01]x");
        assert_eq!(p.lines.len(), 1);
        assert_eq!(p.lines[0].text, "x");
    }

    #[test]
    fn collects_unsynced_lines() {
        let p = parse_lrc("hello\nworld");
        assert!(p.lines.is_empty());
        assert_eq!(p.unsynced, vec!["hello", "world"]);
    }

    #[test]
    fn empty_text_yields_nothing() {
        let p = parse_lrc("");
        assert!(p.lines.is_empty());
        assert!(p.unsynced.is_empty());
    }

    #[test]
    fn active_index_tracks_position() {
        let p = parse_lrc("[00:10]a\n[00:20]b\n[00:30]c");
        assert_eq!(p.active_index(Duration::from_millis(5000)), None);
        assert_eq!(p.active_index(Duration::from_millis(10000)), Some(0));
        assert_eq!(p.active_index(Duration::from_millis(25000)), Some(1));
        assert_eq!(p.active_index(Duration::from_millis(100000)), Some(2));
    }

    #[test]
    fn parses_timespan() {
        assert_eq!(
            parse_timespan("00:01:30.5000000"),
            Some(Duration::from_millis(90500))
        );
        assert_eq!(
            parse_timespan("00:00:05.3700000"),
            Some(Duration::from_millis(5370))
        );
        assert_eq!(parse_timespan("01:00:00"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_timespan("bad"), None);
    }

    #[test]
    fn parses_time_entries_sorted() {
        let entries = vec![
            LyricEntry {
                time: "00:00:10.1200000".into(),
                text: "Second".into(),
            },
            LyricEntry {
                time: "00:00:05.3700000".into(),
                text: "First".into(),
            },
        ];
        let p = parse_time_entries(&entries);
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.lines[0].text, "First");
        assert_eq!(p.lines[0].start, Duration::from_millis(5370));
        assert_eq!(p.lines[1].text, "Second");
        assert_eq!(p.lines[0].end, Duration::from_millis(10120));
    }
}
