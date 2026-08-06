use std::path::PathBuf;
use std::sync::OnceLock;
use serde::{Deserialize, Serialize};
use crate::theme::SelectableTheme;

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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "defaults::volume")]
    pub volume: f32,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub looping: bool,

    #[serde(default)]
    pub theme: SelectableTheme,
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
        }
    }
}