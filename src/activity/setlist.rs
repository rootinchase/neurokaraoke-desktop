use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct SetlistActivity {
    pub setlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub selected_setlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
    pub current_year: Arc<Mutex<u32>>,
}

impl SetlistActivity {
    fn get_cache_path(year: u32) -> std::path::PathBuf {
        crate::cache::cache_dir().join(format!("setlists_{}.ron", year))
    }

    pub fn new(songs: LazySongDatabase) -> Self {
        let setlists = Arc::new(Mutex::new(LoadingState::Loading));
        let current_year = Arc::new(Mutex::new(0));

        let s = setlists.clone();
        let songs_clone = songs.clone();
        tokio::spawn(async move {
            let cache_path = Self::get_cache_path(0);
            
            // Try loading from cache
            if let Ok(data) = std::fs::read(&cache_path) {
                if let Ok(playlists) = ron::de::from_bytes::<Vec<Playlist>>(&data) {
                    *s.lock().await = LoadingState::Loaded(playlists);
                    return;
                }
            }

            // Fallback to network
            match songs_clone.get_official_setlists(0).await {
                Ok(data) => {
                    *s.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ron::ser::to_string_pretty(&data, Default::default()).unwrap(),
                    );
                }
                Err(err) => {
                    let mut s_lock = s.lock().await;
                    if matches!(*s_lock, LoadingState::Loading) {
                        *s_lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }
        });

        Self {
            setlists,
            selected_setlist: Arc::new(Mutex::new(None)),
            songs,
            current_year,
        }
    }
    
    pub fn set_year(&self, year: u32) {
        let s = self.setlists.clone();
        let songs = self.songs.clone();
        let current_year = self.current_year.clone();
        
        tokio::spawn(async move {
            *s.lock().await = LoadingState::Loading;
            *current_year.lock().await = year;
            
            let cache_path = Self::get_cache_path(year);
            
            // Try loading from cache
            if let Ok(data) = tokio::fs::read(&cache_path).await {
                if let Ok(playlists) = ron::de::from_bytes::<Vec<Playlist>>(&data) {
                    *s.lock().await = LoadingState::Loaded(playlists);
                    return;
                }
            }

            // Fallback to network
            match songs.get_official_setlists(year).await {
                Ok(data) => {
                    *s.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ron::ser::to_string_pretty(&data, Default::default()).unwrap(),
                    ).await;
                }
                Err(err) => {
                    *s.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
        });
    }

    pub fn select_setlist(&self, id: Uuid) {
        let selected = self.selected_setlist.clone();
        let songs = self.songs.clone();

        // Spawn a task to update the state asynchronously, avoiding blocking the UI thread.
        tokio::spawn(async move {
            *selected.lock().await = Some(LoadingState::Loading);
            
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
