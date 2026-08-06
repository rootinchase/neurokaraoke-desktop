use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use tokio::sync::Mutex;
use crate::theme::SelectableTheme;
use crate::util::AsArcMutex;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init_config_dir() {
    let dir = dirs::config_dir().expect("config directory should exist").join("neurokaraoke-desktop");
    std::fs::create_dir_all(&dir).unwrap();
    CONFIG_DIR.set(dir).expect("CONFIG_DIR already initialized");
}

pub fn config_dir() -> &'static PathBuf {
    CONFIG_DIR.get().expect("CONFIG_DIR not initialized")
}
pub fn config_file() -> PathBuf { config_dir().join("config.ron") }

mod defaults {
    pub fn volume() -> f32 { 0.5 }

    pub fn cache_expiration_secs() -> u64 { 24 * 60 * 60 }
    pub fn cache_sweep_interval_secs() -> u64 { 60 }
    pub fn cache_size_limit_mb() -> u64 { 1024 }
    pub fn framerate_when_not_focused() -> f32 { 1.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "defaults::cache_expiration_secs")]
    pub cache_expiration_secs: u64,
    #[serde(default = "defaults::cache_sweep_interval_secs")]
    pub cache_sweep_interval_secs: u64,
    #[serde(default = "defaults::cache_size_limit_mb")]
    pub cache_size_limit_mb: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            cache_expiration_secs: defaults::cache_expiration_secs(),
            cache_sweep_interval_secs: defaults::cache_sweep_interval_secs(),
            cache_size_limit_mb: defaults::cache_size_limit_mb(),
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
    pub looping: bool,

    #[serde(default)]
    pub theme: SelectableTheme,

    #[serde(default)]
    #[serde_as(as = "AsArcMutex<CacheConfig>")]
    pub cache: Arc<Mutex<CacheConfig>>,

    #[serde(default = "defaults::framerate_when_not_focused")]
    pub framerate_when_not_focused: f32,
}

impl Config {
    pub fn read() -> anyhow::Result<Self> {
        Ok(ron::de::from_bytes(std::fs::read(config_file())?.as_slice())?)
    }

    pub fn write(&self) -> anyhow::Result<()> {
        std::fs::write(config_file(), ron::ser::to_string_pretty(self, Default::default())?.as_bytes())?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: defaults::volume(),
            shuffle: false,
            looping: false,

            theme: Default::default(),

            cache: Default::default(),

            framerate_when_not_focused: defaults::framerate_when_not_focused(),
        }
    }
}