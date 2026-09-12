use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;


pub struct PlaylistActivity {
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub selected_playlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
}


impl PlaylistActivity {
    pub fn new(songs: LazySongDatabase, is_personal: bool) -> Self {
        let cache_file = if is_personal {
            "my_playlists.ron"
        } else {
            "playlists.ron"
        };
        let cache_path = crate::cache::cache_dir().join(cache_file);
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
            let result = if is_personal {
                songs_clone.get_user_playlists().await
            } else {
                Self::fetch_all_public_playlists(&songs_clone).await
            };

            match result {
                Ok(data) => {
                    *p.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ron::ser::to_string_pretty(&data, Default::default()).unwrap(),
                    )
                    .await;
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
            playlists,
            selected_playlist: Arc::new(Mutex::new(None)),
            songs,
        }
    }


    async fn fetch_all_public_playlists(songs: &LazySongDatabase) -> anyhow::Result<Vec<Playlist>> {
        let mut all_playlists = Vec::new();
        let mut start_index = 0;
        let page_size = 30;

        loop {
            let batch = songs.get_public_playlists(None, false, start_index, page_size).await?;
            if batch.is_empty() {
                break;
            }
            all_playlists.extend(batch);
            start_index += page_size;
        }

        Ok(all_playlists)
    }

    #[allow(dead_code)]
    pub fn fetch_public_playlists(&self) {
        let p = self.playlists.clone();
        let songs = self.songs.clone();

        tokio::spawn(async move {
            *p.lock().await = LoadingState::Loading;
            
            match Self::fetch_all_public_playlists(&songs).await {
                Ok(data) => {
                    *p.lock().await = LoadingState::Loaded(data);
                }
                Err(err) => {
                    *p.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
        });
    }

    pub fn select_playlist(&self, id: Uuid) {
        let selected = self.selected_playlist.clone();
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
