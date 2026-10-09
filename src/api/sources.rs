use anyhow::anyhow;
use url::Url;

/// Server-side validation messages reproduced verbatim, so the client fails with the
/// same wording the web app shows (`POST /api/user/song/download-from-url`).
pub const ERR_INVALID_URL: &str = "Invalid URL format";
pub const ERR_UNSUPPORTED_PLATFORM: &str =
    "Unsupported platform. Only YouTube, Bilibili and Discord attachments are supported.";
pub const ERR_INVALID_YOUTUBE_URL: &str = "Invalid YouTube URL. Must be a video or playlist.";

/// A recognised upstream source for `POST /api/user/song/download-from-url`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    YouTubeVideo { video_id: String },
    YouTubePlaylist { list_id: String },
    BilibiliVideo { video_id: String },
    DiscordAttachment { url: String },
}

/// YouTube video ids are exactly 11 URL-safe characters.
pub fn is_video_id(candidate: &str) -> bool {
    candidate.len() == 11
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validates a source URL offline, mirroring the server's fast checks: the URL must
/// parse, the host must be YouTube, Bilibili, or a Discord attachment, and a YouTube
/// URL must point at a video or a playlist. Availability, age-restriction, and privacy
/// are only knowable by the server, which answers with
/// `"Failed to retrieve video information. …"`.
pub fn classify_source_url(raw: &str) -> anyhow::Result<SourceKind> {
    let raw = raw.trim();
    let parsed = Url::parse(raw).map_err(|_| anyhow!(ERR_INVALID_URL))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(anyhow!(ERR_INVALID_URL));
    }

    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    // `www.` and `m.` prefixes are aliases of the canonical host.
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let host = if host == "m.youtube.com" {
        "youtube.com".to_string()
    } else {
        host
    };

    match host.as_str() {
        "youtu.be" => {
            let id = first_segment(&parsed);
            if is_video_id(&id) {
                Ok(SourceKind::YouTubeVideo { video_id: id })
            } else {
                Err(anyhow!(ERR_INVALID_YOUTUBE_URL))
            }
        }
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => classify_youtube(&parsed),
        "bilibili.com" | "b23.tv" => classify_bilibili(&parsed),
        "cdn.discordapp.com" | "media.discordapp.net" => classify_discord(&parsed),
        _ => Err(anyhow!(ERR_UNSUPPORTED_PLATFORM)),
    }
}

fn first_segment(parsed: &Url) -> String {
    parsed
        .path_segments()
        .and_then(|mut segments| segments.next())
        .unwrap_or_default()
        .to_string()
}

fn query_value(parsed: &Url, key: &str) -> String {
    parsed
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.to_string())
        .unwrap_or_default()
}

fn segments(parsed: &Url) -> Vec<String> {
    parsed
        .path_segments()
        .map(|s| s.map(str::to_string).collect())
        .unwrap_or_default()
}

/// Handles `watch?v=`, `shorts/`, `live/`, `embed/`, `v/`, and `playlist?list=`.
fn classify_youtube(parsed: &Url) -> anyhow::Result<SourceKind> {
    let segments = segments(parsed);
    match segments.first().map(String::as_str) {
        Some("watch") => {
            let video_id = query_value(parsed, "v");
            if is_video_id(&video_id) {
                return Ok(SourceKind::YouTubeVideo { video_id });
            }
            let list_id = query_value(parsed, "list");
            if !list_id.is_empty() {
                return Ok(SourceKind::YouTubePlaylist { list_id });
            }
            Err(anyhow!(ERR_INVALID_YOUTUBE_URL))
        }
        Some("playlist") => {
            let list_id = query_value(parsed, "list");
            if list_id.is_empty() {
                Err(anyhow!(ERR_INVALID_YOUTUBE_URL))
            } else {
                Ok(SourceKind::YouTubePlaylist { list_id })
            }
        }
        Some("shorts") | Some("live") | Some("embed") | Some("v") => {
            let video_id = segments.get(1).cloned().unwrap_or_default();
            if is_video_id(&video_id) {
                Ok(SourceKind::YouTubeVideo { video_id })
            } else {
                Err(anyhow!(ERR_INVALID_YOUTUBE_URL))
            }
        }
        // Bare host, channel pages, `/tv`, `/gaming`, …
        _ => Err(anyhow!(ERR_INVALID_YOUTUBE_URL)),
    }
}

/// Handles `/video/BV…`, `/video/av…`, and the `b23.tv` short links, which the server
/// resolves itself.
fn classify_bilibili(parsed: &Url) -> anyhow::Result<SourceKind> {
    let segments = segments(parsed);
    if parsed.host_str().map(|h| h.to_ascii_lowercase()) == Some("b23.tv".to_string()) {
        let short = segments.first().cloned().unwrap_or_default();
        if short.is_empty() {
            return Err(anyhow!(ERR_INVALID_URL));
        }
        return Ok(SourceKind::BilibiliVideo { video_id: short });
    }

    if segments.first().map(String::as_str) == Some("video") {
        let id = segments.get(1).cloned().unwrap_or_default();
        let is_bv = id.starts_with("BV")
            && id.len() >= 11
            && id[2..].chars().all(|c| c.is_ascii_alphanumeric());
        let is_av = id.starts_with("av") && id[2..].chars().all(|c| c.is_ascii_digit());
        if is_bv || is_av {
            return Ok(SourceKind::BilibiliVideo { video_id: id });
        }
    }
    Err(anyhow!(ERR_INVALID_URL))
}

/// Discord attachments look like
/// `https://cdn.discordapp.com/attachments/{channel}/{message}/{filename}`.
fn classify_discord(parsed: &Url) -> anyhow::Result<SourceKind> {
    let segments = segments(parsed);
    if segments.first().map(String::as_str) == Some("attachments") && segments.len() >= 3 {
        Ok(SourceKind::DiscordAttachment {
            url: parsed.as_str().to_string(),
        })
    } else {
        Err(anyhow!(ERR_INVALID_URL))
    }
}
