use crate::audio::types::LoopMode;
use crate::config::config_dir;
use serde::{Deserialize, Serialize};
use std::fs::{read_to_string, remove_file, write};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppState {
    pub current_song_uuid: Option<Uuid>,
    pub current_position_secs: u64,
    pub playlist: Option<Arc<[Uuid]>>,
    pub playlist_name: Option<String>,
    pub url_playlist: Option<Arc<[crate::api::SongDTO]>>,
    pub volume: f32,
    pub shuffle: bool,
    pub loop_mode: LoopMode,
}

pub fn save_app_state(state: &AppState) -> anyhow::Result<()> {
    let path = config_dir().join("state.ron");
    let content = ron::ser::to_string_pretty(state, ron::ser::PrettyConfig::default())?;
    write(path, content)?;
    Ok(())
}

pub fn load_app_state() -> Option<AppState> {
    let path = config_dir().join("state.ron");
    let content = read_to_string(path).ok()?;
    ron::from_str(&content).ok()
}

pub fn clear_app_state() -> anyhow::Result<()> {
    let path = config_dir().join("state.ron");
    if path.exists() {
        remove_file(path)?;
    }
    Ok(())
}
