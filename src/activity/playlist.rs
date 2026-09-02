use crate::api::{Playlist, PlaylistDetail, LazySongDatabase, LoadingState};
use eframe::egui::{Context};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct PlaylistActivity {
    pub ctx: Context,
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub selected_playlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
}

impl PlaylistActivity {
    pub fn new(ctx: Context, songs: LazySongDatabase) -> Self {
        let cache_path = crate::cache::cache_dir().join("playlists.ron");
        let cached_playlists = std::fs::read(&cache_path)
            .ok()
            .and_then(|data| ron::de::from_bytes(&data).ok());

        let playlists = Arc::new(Mutex::new(if let Some(data) = cached_playlists {
            LoadingState::Loaded(data)
        } else {
            LoadingState::Loading
        }));

        let p = playlists.clone();
        let songs_clone = songs.clone();
        tokio::spawn(async move {
            match songs_clone.get_public_playlists().await {
                Ok(data) => {
                    *p.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(cache_path, ron::ser::to_string_pretty(&data, Default::default()).unwrap()).await;
                }
                Err(err) => {
                    // Only error out if we don't have cached data
                    let mut p_lock = p.lock().await;
                    if matches!(*p_lock, LoadingState::Loading) {
                        *p_lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }
        });

        Self { 
            ctx, 
            playlists, 
            selected_playlist: Arc::new(Mutex::new(None)),
            songs 
        }
    }
    // ...

    pub fn select_playlist(&self, id: Uuid) {
        let selected = self.selected_playlist.clone();
        let songs = self.songs.clone();
        
        *selected.blocking_lock() = Some(LoadingState::Loading);
        
        tokio::spawn(async move {
            match songs.get_playlist_details(id).await {
                Ok(data) => {
                    *selected.lock().await = Some(LoadingState::Loaded(data));
                }
                Err(err) => {
                    *selected.lock().await = Some(LoadingState::Failed(Arc::new(err)));
                }
            }
        });
    }
}
