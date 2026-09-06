use crate::api::{SongDTO, LazySongDatabase, LoadingState};
use eframe::egui::{Context};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;
use crate::{debug_log, Player};
use tokio::runtime::Runtime;

pub struct FavoritesActivity {
    pub ctx: Context,
    pub songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub is_fetching: Arc<AtomicBool>,
}

impl FavoritesActivity {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            songs: Arc::new(Mutex::new(LoadingState::Loading)),
            is_fetching: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn fetch_favorites(&self, db: &LazySongDatabase) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }

        let s = self.songs.clone();
        let ctx = self.ctx.clone();
        let db = db.clone();
        let is_fetching = self.is_fetching.clone();

        *s.blocking_lock() = LoadingState::Loading;

        tokio::spawn(async move {
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

    pub fn play_favorites(&self, player: &Player) {
        debug_log!("Playing favorites");
        let songs_arc = self.songs.clone();
        let player = player.clone();

        tokio::spawn(async move {
            debug_log!("Async playback task started");
            let songs_lock = songs_arc.lock().await;
            
            if let LoadingState::Loaded(ref songs) = *songs_lock {
                debug_log!("LoadedState:Loaded, songs found: {}", songs.len());
                let mut pl: Vec<Uuid> = Vec::new();
                let mut valid_songs_to_play: Vec<SongDTO> = Vec::new();

                for song in songs {
                    if song.is_valid() {
                        pl.push(Uuid::new_v4());
                        valid_songs_to_play.push(song.clone());
                    }
                }

                if pl.is_empty() {
                    debug_log!("No valid songs to play");
                    return;
                }

                debug_log!("Setting playlist and starting playback");
                player.clear_playlist();
                player.playlist(Some(pl.clone().into()));
                player.url_playlist(Some(valid_songs_to_play.clone().into()));

                if let Some(first_song) = valid_songs_to_play.first() {
                    player.url_playback(Some(pl[0]), first_song.clone(), Player::play);
                }
            } else {
                debug_log!("Songs not loaded or failed to load");
            }
        });
    }
}