use crate::activity::{SortOption, render_playlist_art_advanced, render_playlist_box, search};
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail, SongDTO};
use crate::utilities::cache::{PersistentMediaCache, cache_dir};
use crate::utilities::util::{select_playlist_by_id, sort_items};

use crate::debug_log;
use dashmap::DashMap;
use eframe::egui::{ScrollArea, TextEdit, Context, Ui};
use ron::de;
use ron::ser;
use std::fs::read;
use std::cell::RefCell;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct PlaylistActivity {
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub playlist_details: Arc<DashMap<Uuid, LoadingState<PlaylistDetail>>>,
    pub selected_playlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
    pub cache: Arc<PersistentMediaCache>,
    pub ctx: Context,
    pub rt: Arc<Runtime>,
    pub client: reqwest::Client,
    pub is_personal: bool,
    pub has_more: Arc<Mutex<bool>>,
    pub loading_more: Arc<Mutex<bool>>,
}

impl PlaylistActivity {
    pub fn new(
        songs: LazySongDatabase,
        is_personal: bool,
        cache: Arc<PersistentMediaCache>,
        ctx: Context,
        rt: Arc<Runtime>,
        client: reqwest::Client,
    ) -> Self {
        let cache_file = if is_personal {
            "my_playlists.ron"
        } else {
            "playlists.ron"
        };
        let cache_path = cache_dir().join(cache_file);
        let cached_playlists = read(&cache_path)
            .ok()
            .and_then(|data| de::from_bytes(&data).ok());

        let playlists = Arc::new(Mutex::new(if let Some(data) = cached_playlists {
            LoadingState::Loaded(data)
        } else {
            LoadingState::Loading
        }));

        let has_more = Arc::new(Mutex::new(!is_personal));
        let loading_more = Arc::new(Mutex::new(false));

        let p = playlists.clone();
        let songs_clone = songs.clone();
        let hm = has_more.clone();
        tokio::spawn(async move {
            let result = if is_personal {
                songs_clone.get_user_playlists().await
            } else {
                match songs_clone.get_public_playlists(None, false, 0, 20).await {
                    Ok(batch) => {
                        if batch.len() < 20 {
                            *hm.lock().await = false;
                        }
                        Ok(batch)
                    }
                    Err(e) => Err(e),
                }
            };

            match result {
                Ok(data) => {
                    *p.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ser::to_string_pretty(&data, Default::default()).unwrap(),
                    )
                        .await;
                }
                Err(err) => {
                    let mut p_lock = p.lock().await;
                    if matches!(*p_lock, LoadingState::Loading) {
                        *p_lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }
        });

        Self {
            playlists,
            playlist_details: Arc::new(DashMap::new()),
            selected_playlist: Arc::new(Mutex::new(None)),
            songs,
            cache,
            ctx,
            rt,
            client,
            is_personal,
            has_more,
            loading_more,
        }
    }

    pub async fn clear(&self) {
        *self.playlists.lock().await = LoadingState::Loading;
        *self.selected_playlist.lock().await = None;
        *self.has_more.lock().await = !self.is_personal;
        *self.loading_more.lock().await = false;
        debug_log!("🧹 [Playlist Activity] Resetting memory tables back to pristine state.");
    }

    pub fn load_more(&self) {
        if self.is_personal {
            return;
        }
        let loading = self.loading_more.clone();
        let has_more = self.has_more.clone();
        let playlists = self.playlists.clone();
        let songs = self.songs.clone();
        let cache_path = cache_dir().join("playlists.ron");
        let ctx = self.ctx.clone();

        if *loading.blocking_lock() || !*has_more.blocking_lock() {
            return;
        }

        *loading.blocking_lock() = true;
        debug_log!("🚀 [PlaylistActivity] Triggered load_more() to fetch next batch of public playlists...");

        tokio::spawn(async move {
            let current_len = match &*playlists.lock().await {
                LoadingState::Loaded(list) => list.len(),
                _ => 0,
            };

            debug_log!("🌐 [PlaylistActivity] Fetching public playlists starting at index {}", current_len);
            match songs.get_public_playlists(None, false, current_len as u64, 20).await {
                Ok(batch) => {
                    let is_empty = batch.is_empty();
                    let len = batch.len();
                    debug_log!("✅ [PlaylistActivity] Fetched batch of {} playlists", len);
                    let mut lock = playlists.lock().await;
                    if let LoadingState::Loaded(list) = &mut *lock {
                        list.extend(batch);
                        let _ = tokio::fs::write(
                            cache_path,
                            ser::to_string_pretty(&list, Default::default()).unwrap(),
                        )
                            .await;
                    }
                    if is_empty || len < 20 {
                        *has_more.lock().await = false;
                        debug_log!("🏁 [PlaylistActivity] Reached end of public playlists.");
                    }
                }
                Err(e) => {
                    debug_log!("❌ [PlaylistActivity] Error fetching public playlists batch: {}", e);
                    *has_more.lock().await = false;
                }
            }

            *loading.lock().await = false;
            ctx.request_repaint();
        });
    }

    pub fn select_playlist(&self, id: Uuid) {
        select_playlist_by_id(id, self.selected_playlist.clone(), self.songs.clone());
    }

    pub fn render(
        &self,
        ui: &mut Ui,
        playlist_search: &mut Option<&mut String>,
        current_sort: &mut SortOption,
        current_sort_desc: &mut bool,
        mut play_playlist: impl FnMut(&PlaylistDetail),
        play_song: impl FnMut(Vec<SongDTO>, Uuid) + 'static,
        is_personal: bool,
        _creator_name: Option<String>,
    ) {
        let play_song = RefCell::new(play_song);
        ui.vertical(|ui| {
            if let Some(search) = playlist_search {
                ui.horizontal(|ui| {
                    ui.label(if is_personal {
                        "My Playlists:"
                    } else {
                        "Public Playlists:"
                    });
                    ui.add(TextEdit::singleline(*search));
                });
            } else {
                ui.label(if is_personal {
                    "My Playlists:"
                } else {
                    "Public Playlists:"
                });
            }

            // Single lock access per frame
            let playlists_lock = self.playlists.try_lock();

            match playlists_lock {
                Ok(playlists) => {
                    match &*playlists {
                        LoadingState::Loaded(playlists) => {
                            let mut sorted_playlists: Vec<_> = if let Some(search) = playlist_search
                            {
                                let criteria = search::parse_query(search, true);
                                playlists
                                    .iter()
                                    .filter(|p| {
                                        let name_match = p
                                            .name
                                            .to_lowercase()
                                            .contains(&criteria.title.to_lowercase());
                                        let creator_match =
                                            criteria.creator.as_ref().map_or(true, |q| {
                                                p.creator.to_lowercase().contains(&q.to_lowercase())
                                            });
                                        name_match && creator_match
                                    })
                                    .cloned()
                                    .collect()
                            } else {
                                playlists.clone()
                            };

                            sort_items(
                                &mut sorted_playlists,
                                current_sort,
                                current_sort_desc,
                                |p| p.name.to_string(),
                                |p| p.song_count.try_into().unwrap(),
                                |p| p.play_count.try_into().unwrap(),
                                |p| {
                                    p.updated_at
                                        .as_deref()
                                        .or(p.created_at.as_deref())
                                        .map(|s| s.to_string())
                                },
                            );

                            ScrollArea::vertical().show(ui, |ui| {
                                for (index, playlist) in sorted_playlists.iter().enumerate() {
                                    // Automatically load more when reaching the 5th item from the end
                                    if !self.is_personal && index + 5 >= sorted_playlists.len() {
                                        self.load_more();
                                    }
                                    // Fetch detail if needed
                                    if !self.playlist_details.contains_key(&playlist.id) {
                                        self.playlist_details.insert(playlist.id, LoadingState::Loading);
                                        let p_id = playlist.id;
                                        let p_songs = self.songs.clone();
                                        let p_details = self.playlist_details.clone();
                                        self.rt.spawn(async move {
                                            match p_songs.get_playlist_details(p_id).await {
                                                Ok(detail) => {
                                                    p_details.insert(p_id, LoadingState::Loaded(detail));
                                                }
                                                Err(err) => {
                                                    p_details.insert(p_id, LoadingState::Failed(Arc::new(err)));
                                                }
                                            }
                                        });
                                    }

                                    if let Some(detail_state) = self.playlist_details.get(&playlist.id) {
                                        match &*detail_state {
                                            LoadingState::Loaded(detail) => {
                                                render_playlist_box(
                                                    ui,
                                                    &crate::theme::ThemeManager::new(crate::config::Config::read().unwrap_or_default().theme.as_theme()),
                                                    &format!("playlist_box_{}", playlist.id),
                                                    &detail.name,
                                                    &playlist.creator,
                                                    detail,
                                                    &mut |songs, song_id| {
                                                        play_song.borrow_mut()(songs, song_id)
                                                    },
                                                    |ui| {
                                                        render_playlist_art_advanced(
                                                            ui,
                                                            &self.cache,
                                                            &self.ctx,
                                                            &self.rt,
                                                            &self.client,
                                                            playlist,
                                                            &detail.songs,
                                                            eframe::egui::Vec2::new(150.0, 150.0),
                                                        );
                                                    },
                                                );

                                                if ui.button("Play Playlist").clicked() {
                                                    play_playlist(detail);
                                                }
                                            }
                                            LoadingState::Loading => {
                                                ui.label(format!("Loading {}...", playlist.name));
                                            }
                                            LoadingState::Failed(err) => {
                                                ui.label(format!(
                                                    "Error loading playlist {}: {}",
                                                    playlist.name, err
                                                ));
                                            }
                                        }
                                    }
                                }

                                // Load more button at the bottom of public playlists
                                if !self.is_personal && playlist_search.is_none() {
                                    ui.add_space(10.0);
                                    ui.horizontal(|ui| {
                                        let has_more = *self.has_more.blocking_lock();
                                        let loading_more = *self.loading_more.blocking_lock();
                                        if has_more {
                                            if loading_more {
                                                ui.spinner();
                                                ui.label("Loading more playlists...");
                                            } else if ui.button("Load More Playlists").clicked() {
                                                self.load_more();
                                            }
                                        } else {
                                            ui.label("All public playlists loaded.");
                                        }
                                    });
                                    ui.add_space(20.0);
                                }
                            });
                        }
                        LoadingState::Loading => {
                            ui.label("Loading...");
                        }
                        LoadingState::Failed(err) => {
                            ui.label(format!("Error loading playlists: {}", err));
                        }
                    }
                }
                Err(_) => {
                    ui.label("Loading...");
                }
            }
        });
    }
}

