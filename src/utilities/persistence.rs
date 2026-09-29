use crate::audio::Player;
use crate::audio::types::LoopMode;
use crate::config::{Config, config_dir};
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

pub async fn handle_signals(player: Player, config: Config) {
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        } else {
            let _ = ctrl_c.await;
        }
    }

    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
    }

    let _ = tokio::task::spawn_blocking(move || {
        if let Some(state) = player.get_playback_state() {
            let app_state = AppState {
                current_song_uuid: Some(state.song()),
                current_position_secs: state.position().as_secs(),
                playlist: player.get_playlist(),
                playlist_name: player.get_playlist_name(),
                url_playlist: player.get_url_playlist(),
                volume: player.get_volume(),
                shuffle: player.get_shuffle(),
                loop_mode: player.get_loop_mode(),
            };
            let _ = save_app_state(&app_state);
        }

        let _ = config.write_config();
    })
    .await;

    std::process::exit(0);
}
