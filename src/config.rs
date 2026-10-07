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
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_expiration_secs: defaults::cache_expiration_secs(),
            cache_sweep_interval_secs: defaults::cache_sweep_interval_secs(),
            cache_size_limit_mb: defaults::cache_size_limit_mb(),
            song_database_update_interval_secs: defaults::song_database_update_interval_secs(),
            playlist_cache_ttl_secs: defaults::playlist_cache_ttl_secs(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RadioUrl {
    #[default]
    Mp3320Kbps,
    Opus,
}

impl RadioUrl {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Mp3320Kbps => "320kbps MP3",
            Self::Opus => "Opus",
        }
    }

    pub fn url(&self) -> &'static str {
        match self {
            Self::Mp3320Kbps => "https://radio.twinskaraoke.com/listen/neuro_21/radio.mp3",
            Self::Opus => "https://radio.twinskaraoke.com/listen/neuro_21/radio.ogg",
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
