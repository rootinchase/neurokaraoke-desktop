use crate::activity::{SortOption, render_playlist_art_advanced, render_playlist_box, search};
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail, SongDTO};
use crate::config::Config;
use crate::theme::ThemeManager;
use crate::utilities::cache::{PersistentMediaCache, playlist_cache_dir};
use crate::utilities::util::{sort_items, spawn_fetch_playlist_details};

use crate::debug_log;
use dashmap::DashMap;
use eframe::egui::{Context, ScrollArea, TextEdit, Ui, Vec2};
use reqwest::Client;
use ron::de;
use ron::ser;
use std::cell::RefCell;
use std::fs::read;
use std::sync::Arc;
use tokio::fs::write;
use tokio::runtime::Runtime;
use tokio::spawn;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct PlaylistActivity {
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub all_playlists: Arc<Mutex<Option<Vec<Playlist>>>>,
    pub playlist_details: Arc<DashMap<Uuid, LoadingState<PlaylistDetail>>>,
    pub selected_playlist: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    pub songs: LazySongDatabase,
    pub cache: Arc<PersistentMediaCache>,
    pub ctx: Context,
    pub rt: Arc<Runtime>,
    pub client: Client,
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
        client: Client,
    ) -> Self {
        let cache_file = if is_personal {
            "my_playlists.ron"
        } else {
            "playlists.ron"
        };
        let cache_path = playlist_cache_dir().join(cache_file);
        let cached_playlists: Option<Vec<Playlist>> = read(&cache_path)
            .ok()
            .and_then(|data| de::from_bytes(&data).ok());

        let all_playlists = Arc::new(Mutex::new(cached_playlists.clone()));

        let initial_playlists = if let Some(ref data) = cached_playlists {
            data.iter().take(20).cloned().collect()
        } else {
            Vec::new()
        };

        let playlists = Arc::new(Mutex::new(if !initial_playlists.is_empty() {
            LoadingState::Loaded(initial_playlists)
        } else {
            LoadingState::Loading
        }));

        let has_more = Arc::new(Mutex::new(
            cached_playlists.as_ref().map_or(true, |d| d.len() > 20),
        ));
        let loading_more = Arc::new(Mutex::new(false));

        let p = playlists.clone();
        let ap = all_playlists.clone();
        let songs_clone = songs.clone();
        let hm = has_more.clone();
        let ctx_clone = ctx.clone();
        let cp_clone = cache_path.clone();
        spawn(async move {
            let result = if is_personal {
                songs_clone.get_user_playlists().await
            } else {
                songs_clone.get_public_playlists(None, false, 0, 500).await
            };

            match result {
                Ok(data) => {
                    *ap.lock().await = Some(data.clone());
                    let initial: Vec<_> = data.iter().take(20).cloned().collect();
                    *p.lock().await = LoadingState::Loaded(initial);
                    *hm.lock().await = data.len() > 20;

                    if let Some(parent) = cp_clone.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let _ = write(
                        cp_clone,
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
            ctx_clone.request_repaint();
        });

        Self {
            playlists,
            all_playlists,
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
        *self.all_playlists.lock().await = None;
        *self.selected_playlist.lock().await = None;
        *self.has_more.lock().await = true;
        *self.loading_more.lock().await = false;
        debug_log!("🧹 [Playlist Activity] Resetting memory tables back to pristine state.");
    }

    pub fn load_more(&self) {
        let loading = self.loading_more.clone();
        let has_more = self.has_more.clone();
        let playlists = self.playlists.clone();
        let all_playlists = self.all_playlists.clone();
        let songs = self.songs.clone();
        let cache_path = playlist_cache_dir().join(if self.is_personal {
            "my_playlists.ron"
        } else {
            "playlists.ron"
        });
        let ctx = self.ctx.clone();
        let is_personal = self.is_personal;

        debug_log!(
            "🚀 [PlaylistActivity] load_more() called. has_more={}, loading_more={}",
            *has_more.blocking_lock(),
            *loading.blocking_lock()
        );

        if *loading.blocking_lock() || !*has_more.blocking_lock() {
            debug_log!(
                "⚠️ [PlaylistActivity] load_more() aborted: loading={}, has_more={}",
                *loading.blocking_lock(),
                *has_more.blocking_lock()
            );
            return;
        }

        *loading.blocking_lock() = true;
        debug_log!("🚀 [PlaylistActivity] Triggered load_more() execution...");

        spawn(async move {
            let mut all_opt = all_playlists.lock().await;
            let all = if let Some(ref list) = *all_opt {
                list.clone()
            } else {
                let fetch_result = if is_personal {
                    songs.get_user_playlists().await
                } else {
                    songs.get_public_playlists(None, false, 0, 500).await
                };

                match fetch_result {
                    Ok(data) => {
                        if let Some(parent) = cache_path.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        let _ = write(
                            cache_path.clone(),
                            ser::to_string_pretty(&data, Default::default()).unwrap(),
                        )
                        .await;
                        *all_opt = Some(data.clone());
                        data
                    }
                    Err(e) => {
                        debug_log!(
                            "❌ [PlaylistActivity] Error fetching playlists in load_more: {}. Trying disk cache fallback.",
                            e
                        );
                        if let Ok(file_data) = tokio::fs::read(&cache_path).await {
                            if let Ok(cached_data) = de::from_bytes::<Vec<Playlist>>(&file_data) {
                                *all_opt = Some(cached_data.clone());
                                cached_data
                            } else {
                                *has_more.lock().await = false;
                                *loading.lock().await = false;
                                ctx.request_repaint();
                                return;
                            }
                        } else {
                            *has_more.lock().await = false;
                            *loading.lock().await = false;
                            ctx.request_repaint();
                            return;
                        }
                    }
                }
            };
            drop(all_opt);

            let current_len = match &*playlists.lock().await {
                LoadingState::Loaded(list) => list.len(),
                _ => 0,
            };
            let next_batch: Vec<_> = all.iter().skip(current_len).take(20).cloned().collect();
            let mut lock = playlists.lock().await;
            if let LoadingState::Loaded(list) = &mut *lock {
                list.extend(next_batch);
                if list.len() >= all.len() {
                    *has_more.lock().await = false;
                }
            }

            *loading.lock().await = false;
            ctx.request_repaint();
        });
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
                    ui.add(TextEdit::singleline(*search));
                    ui.add_space(15.0);
                    ui.label("Sort:");

                    for (opt, label) in [
                        (SortOption::Name, "Name"),
                        (SortOption::Songs, "Songs"),
                        (SortOption::Plays, "Plays"),
                        (SortOption::Date, "Date"),
                    ] {
                        let text = if *current_sort == opt {
                            if *current_sort_desc {
                                format!("{} ↓", label)
                            } else {
                                format!("{} ↑", label)
                            }
                        } else {
                            label.to_string()
                        };

                        if ui.selectable_label(*current_sort == opt, text).clicked() {
                            if *current_sort == opt {
                                *current_sort_desc = !*current_sort_desc;
                            } else {
                                *current_sort = opt;
                                *current_sort_desc = false;
                            }

                            // Apply sort globally to entire list (all_playlists)
                            if let Ok(mut all_opt) = self.all_playlists.try_lock() {
                                if let Some(ref mut all) = *all_opt {
                                    sort_items(
                                        all,
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
                                    if let Ok(mut playlists_guard) = self.playlists.try_lock() {
                                        if let LoadingState::Loaded(list) = &mut *playlists_guard {
                                            let current_len = list.len().max(20);
                                            *list = all.iter().take(current_len).cloned().collect();
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
                ui.add_space(8.0);
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

                            let scroll_output = ScrollArea::vertical().show(ui, |ui| {
                                for (_index, playlist) in sorted_playlists.iter().enumerate() {
                                    spawn_fetch_playlist_details(&self.rt, Some(&self.ctx), &self.songs, &self.playlist_details, playlist.id, playlist.name.clone());

                                    if let Some(detail_state) =
                                        self.playlist_details.get(&playlist.id)
                                    {
                                        match &*detail_state {
                                            LoadingState::Loaded(detail) => {
                                                render_playlist_box(
                                                    ui,
                                                    &ThemeManager::new(
                                                        Config::read()
                                                            .unwrap_or_default()
                                                            .theme
                                                            .as_theme(),
                                                    ),
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
                                                            Vec2::new(150.0, 150.0),
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

                                // Load more button at the bottom of playlists
                                if playlist_search.is_none()
                                    || playlist_search.as_ref().map_or(true, |s| s.is_empty())
                                {
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
                                            ui.label(if is_personal {
                                                "All personal playlists loaded."
                                            } else {
                                                "All public playlists loaded."
                                            });
                                        }
                                    });
                                    ui.add_space(20.0);
                                }
                            });

                            let offset_y = scroll_output.state.offset.y;
                            let content_height = scroll_output.content_size.y;
                            let viewport_height = scroll_output.inner_rect.height();
                            let has_more = *self.has_more.blocking_lock();
                            let loading_more = *self.loading_more.blocking_lock();

                            if has_more
                                && !loading_more
                                && (offset_y > 0.0 || content_height <= viewport_height + 300.0)
                                && offset_y + viewport_height >= content_height - 300.0
                            {
                                debug_log!(
                                    "🚀 [PlaylistActivity] Auto-triggering load_more() from scroll"
                                );
                                self.load_more();
                            }
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
