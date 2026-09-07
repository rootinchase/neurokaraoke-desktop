use crate::api::{Playlist, PlaylistDetail, SongDTO, LazySongDatabase, LoadingState};
use eframe::egui::{Context};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use crate::{debug_log, Player};

pub struct FavoritesActivity {
    pub ctx: Context,
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub selected_playlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub is_fetching: Arc<AtomicBool>,
}

impl FavoritesActivity {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            playlists: Arc::new(Mutex::new(LoadingState::Loading)),
            songs: Arc::new(Mutex::new(LoadingState::Loading)),
            selected_playlist: Arc::new(Mutex::new(None)),
            is_fetching: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn fetch_favorites(&self, db: &LazySongDatabase) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }

        let p = self.playlists.clone();
        let s = self.songs.clone();
        let ctx = self.ctx.clone();
        let db = db.clone();
        let is_fetching = self.is_fetching.clone();

        *p.blocking_lock() = LoadingState::Loading;
        *s.blocking_lock() = LoadingState::Loading;

        tokio::spawn(async move {
            // Fetch playlists
            match db.fetch_favorite_playlists().await {
                Ok(playlists) => {
                    *p.lock().await = LoadingState::Loaded(playlists);
                }
                Err(err) => {
                    *p.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }

            // Fetch songs
            match db.get_favorite_songs().await {
                Ok(songs) => {
                    *s.lock().await = LoadingState::Loaded(songs);
                }
                Err(err) => {
                    *s.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
            
            is_fetching.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    pub fn select_playlist(&self, id: Uuid, db: &LazySongDatabase) {
        let selected = self.selected_playlist.clone();
        let db = db.clone();
        let ctx = self.ctx.clone();
        
        *selected.blocking_lock() = Some(LoadingState::Loading);
        
        tokio::spawn(async move {
            match db.get_playlist_details(id).await {
                Ok(data) => {
                    *selected.lock().await = Some(LoadingState::Loaded(data));
                }
                Err(err) => {
                    *selected.lock().await = Some(LoadingState::Failed(Arc::new(err)));
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn play_favorites(&self, _player: &Player) {
        debug_log!("Playing favorite playlists is not yet implemented for the favorites activity");
    }
}