use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct SetlistActivity {
    pub setlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub selected_setlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
}

impl SetlistActivity {
    pub fn new(songs: LazySongDatabase) -> Self {
        let cache_path = crate::cache::cache_dir().join("setlists.ron");
        let cached_setlists = std::fs::read(&cache_path)
            .ok()
            .and_then(|data| ron::de::from_bytes(&data).ok());

        let setlists = Arc::new(Mutex::new(if let Some(data) = cached_setlists {
            LoadingState::Loaded(data)
        } else {
            LoadingState::Loading
        }));

        let s = setlists.clone();
        let songs_clone = songs.clone();
        tokio::spawn(async move {
            match songs_clone.get_official_setlists().await {
                Ok(data) => {
                    *s.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ron::ser::to_string_pretty(&data, Default::default()).unwrap(),
                    )
                    .await;
                }
                Err(err) => {
                    // Only error out if we don't have cached data
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
        }
    }
    // ...

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
