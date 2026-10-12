use crate::api::AuthContext;
use crate::audio::types::LoopMode;
use crate::theme::SelectableTheme;
use crate::utilities::util::AsArcMutex;
use ron::{de, ser};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use std::fs::{create_dir_all, read, write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::{Arc, OnceLock, RwLock};
use tokio::sync::Mutex;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init_config_dir() {
    let dir = dirs::config_dir()
        .expect("config directory should exist")
        .join("neurokaraoke-desktop");
    create_dir_all(&dir).unwrap();
    CONFIG_DIR.set(dir).expect("CONFIG_DIR already initialized");
}

pub fn config_dir() -> &'static PathBuf {
    CONFIG_DIR.get().expect("CONFIG_DIR not initialized")
}
pub fn config_file() -> PathBuf {
    config_dir().join("config.ron")
}

mod defaults {
    pub fn volume() -> f32 {
        0.5
    }
    pub fn cache_expiration_secs() -> u64 {
        24 * 60 * 60
    }
    pub fn cache_sweep_interval_secs() -> u64 {
        60
    }
    pub fn cache_size_limit_mb() -> u64 {
        1024
    }
    pub fn framerate_when_not_focused() -> f32 {
        1.0
    }
    pub fn song_database_update_interval_secs() -> u64 {
        4 * 60 * 60
    }
    pub fn playlist_cache_ttl_secs() -> u64 {
        12 * 60 * 60
    }
    /// Cap on the in-memory `LazySongDatabase` song map. Entries beyond this are
    /// evicted least-recently-used first, keeping pinned songs (current, queue,
    /// favorites). Songs not in the map re-fetch on demand via `get()`.
    pub fn song_map_max_entries() -> usize {
        2000
    }
    /// Cap on the in-memory `cached_art_paths` artwork reference map. Entries
    /// beyond this are evicted least-recently-used first, and entries whose disk
    /// file has been evicted are dropped.
    pub fn art_path_max_entries() -> usize {
        100
    }
    /// Cap on the in-memory `in_flight` asset map in `PersistentMediaCache`.
    /// This map grows with every downloaded asset (audio, thumbnail, full image)
    /// and is the artwork path cache that must be kept bounded. Entries beyond
    /// this are evicted least-recently-used first; dropped entries re-resolve
    /// from disk on the next access.
    pub fn in_flight_max_entries() -> usize {
        15
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "defaults::cache_expiration_secs")]
    pub cache_expiration_secs: u64,
    #[serde(default = "defaults::cache_sweep_interval_secs")]
    pub cache_sweep_interval_secs: u64,
    #[serde(default = "defaults::cache_size_limit_mb")]
    pub cache_size_limit_mb: u64,
    #[serde(default = "defaults::song_database_update_interval_secs")]
    pub song_database_update_interval_secs: u64,
    #[serde(default = "defaults::playlist_cache_ttl_secs")]
    pub playlist_cache_ttl_secs: u64,
    #[serde(default = "defaults::song_map_max_entries")]
    pub song_map_max_entries: usize,
    #[serde(default = "defaults::art_path_max_entries")]
    pub art_path_max_entries: usize,
    #[serde(default = "defaults::in_flight_max_entries")]
    pub in_flight_max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_expiration_secs: defaults::cache_expiration_secs(),
            cache_sweep_interval_secs: defaults::cache_sweep_interval_secs(),
            cache_size_limit_mb: defaults::cache_size_limit_mb(),
            song_database_update_interval_secs: defaults::song_database_update_interval_secs(),
            playlist_cache_ttl_secs: defaults::playlist_cache_ttl_secs(),
            song_map_max_entries: defaults::song_map_max_entries(),
            art_path_max_entries: defaults::art_path_max_entries(),
            in_flight_max_entries: defaults::in_flight_max_entries(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadioUrl {
    #[default]
    Mp3320Kbps,
    Opus,
    Hls,
}

impl RadioUrl {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Mp3320Kbps => "320kbps MP3",
            Self::Opus => "Opus",
            Self::Hls => "HLS (AAC)",
        }
    }

    pub fn url(&self) -> &'static str {
        match self {
            Self::Mp3320Kbps => "https://radio.twinskaraoke.com/listen/neuro_21/radio.mp3",
            Self::Opus => "https://radio.twinskaraoke.com/listen/neuro_21/radio.ogg",
            Self::Hls => "https://radio.twinskaraoke.com/hls/neuro_21/live.m3u8",
        }
    }

    /// Returns true if this radio URL is an HLS playlist (needs the HLS playback path).
    pub fn is_hls(&self) -> bool {
        matches!(self, Self::Hls)
    }
}

/// HLS tier preference for adaptive bitrate selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PreferredTier {
    /// Estimate bandwidth and pick the highest sustainable tier automatically.
    #[default]
    Auto,
    /// Lock to the lowest tier (lofi, ~52 kbps).
    Lofi,
    /// Lock to the middle tier (midfi, ~105 kbps).
    Midfi,
    /// Lock to the highest tier (hifi, ~211 kbps).
    Hifi,
}

impl PreferredTier {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Lofi => "Lofi",
            Self::Midfi => "Midfi",
            Self::Hifi => "Hifi",
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "defaults::volume")]
    pub volume: f32,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub loop_mode: LoopMode,
    #[serde(default)]
    pub radio_url: RadioUrl,
    #[serde(default)]
    pub preferred_tier: PreferredTier,

    #[serde(default)]
    pub theme: SelectableTheme,

    #[serde(default)]
    #[serde_as(as = "AsArcMutex<CacheConfig>")]
    pub cache: Arc<Mutex<CacheConfig>>,

    #[serde(default = "defaults::framerate_when_not_focused")]
    pub framerate_when_not_focused: f32,

    #[serde(default)]
    pub compact_sidebar: bool,

    /// Stores active user authentication. If None, app runs anonymously.
    #[serde(default)]
    pub auth: Option<AuthContext>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: defaults::volume(),
            shuffle: false,
            loop_mode: LoopMode::None,
            radio_url: Default::default(),
            preferred_tier: PreferredTier::default(),
            theme: Default::default(),
            cache: Default::default(),
            framerate_when_not_focused: defaults::framerate_when_not_focused(),
            compact_sidebar: false,
            auth: None, // Defaults to logged-out state
        }
    }
}

/// Thread-safe lock-free mirror used across concurrent background processes.
#[derive(Clone)]
pub struct SharedConfig {
    pub _volume: Arc<AtomicU32>, // Scaled bits integer representation
    pub shuffle: Arc<AtomicBool>,
    pub loop_mode: Arc<AtomicU32>, // 0: None, 1: One, 2: All
    pub auth_token: Arc<RwLock<Option<Arc<str>>>>,
}

impl Config {
    pub fn read() -> anyhow::Result<Self> {
        Ok(de::from_bytes(read(config_file())?.as_slice())?)
    }

    pub fn write_config(&self) -> anyhow::Result<()> {
        write(
            config_file(),
            ser::to_string_pretty(self, Default::default())?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn write(&self) -> anyhow::Result<()> {
        self.write_config()
    }

    pub fn to_shared(&self) -> SharedConfig {
        let mode_u32 = match self.loop_mode {
            LoopMode::None => 0,
            LoopMode::One => 1,
            LoopMode::All => 2,
        };
        SharedConfig {
            _volume: Arc::new(AtomicU32::new(self.volume.to_bits())),
            shuffle: Arc::new(AtomicBool::new(self.shuffle)),
            loop_mode: Arc::new(AtomicU32::new(mode_u32)),
            auth_token: Arc::new(RwLock::new(self.auth.as_ref().map(|a| a.token.clone()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radio_url_properties() {
        assert_eq!(RadioUrl::Mp3320Kbps.name(), "320kbps MP3");
        assert_eq!(
            RadioUrl::Mp3320Kbps.url(),
            "https://radio.twinskaraoke.com/listen/neuro_21/radio.mp3"
        );
        assert_eq!(RadioUrl::Opus.name(), "Opus");
        assert_eq!(
            RadioUrl::Opus.url(),
            "https://radio.twinskaraoke.com/listen/neuro_21/radio.ogg"
        );
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.volume, 0.5);
        assert!(!config.shuffle);
        assert_eq!(config.loop_mode, LoopMode::None);
        assert_eq!(config.radio_url, RadioUrl::Mp3320Kbps);
        assert!(config.auth.is_none());
    }
}
