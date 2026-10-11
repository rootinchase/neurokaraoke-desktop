use crate::activity::{
    SortOption, cover_art_uri, render_duration_label, render_playlist_art_advanced,
    render_playlist_box, search, visible_range,
};
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail, SongDTO};
use crate::app::fonts;
use crate::config::Config;
use crate::theme::ThemeManager;
use crate::utilities::cache::{PersistentMediaCache, playlist_cache_dir};
use crate::utilities::util::{
    format_added_date, format_play_count, format_total_duration, sort_items,
    spawn_fetch_playlist_details,
};

use crate::debug_log;
use dashmap::DashMap;
use eframe::egui::{
    Align, Area, Button, Context, Frame, Id, Image, Layout, Order, Pos2, Rect, RichText, ScrollArea,
    Sense, TextEdit, Ui, Vec2, include_image,
};
use egui_extras::{Column, TableBuilder};
use reqwest::Client;
use ron::de;
use ron::ser;
use std::cell::RefCell;
use std::collections::HashSet;
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
    pub selected_playlist_id: Arc<Mutex<Option<Uuid>>>,
    pub track_search: Arc<Mutex<String>>,
    pub show_more_menu: Arc<Mutex<bool>>,
    pub more_menu_pos: Arc<Mutex<Option<Pos2>>>,
    /// Playlist ids the user has favorited, shared with `FavoritesActivity` so the
    /// detail view can show and toggle the favorite state.
    pub favorite_playlists: Arc<Mutex<HashSet<Uuid>>>,
    /// Last vertical scroll offset of the playlist grid, used to virtualize the
    /// grid so only visible playlist boxes render cover-art textures. This keeps
    /// egui's texture store bounded (each 512x512 cover is ~1 MB) instead of
    /// registering a texture for every loaded playlist.
    pub last_scroll_y: Arc<Mutex<f64>>,
}

/// An action chosen from the playlist detail "more" (kebab) popup. The caller
/// (`render`) wires these to the corresponding playlist operation. Actions whose
/// server endpoints are not implemented yet (issue #1, 0.5.x) are logged as stubs.
#[derive(Debug, Clone, PartialEq)]
pub enum MoreMenuAction {
    // Backend actions not wired up yet — commented out for this commit.
    // DownloadAll,
    // DownloadSelected,
    // SaveOffline,
    // CreateNewPlaylist,
    // AddToPlaylist(Uuid),
    // UploadYourSongs,
    CopyLink,
    // EditPlaylist,
    // DeletePlaylist,
}

impl PlaylistActivity {
    pub fn new(
        songs: LazySongDatabase,
        is_personal: bool,
        cache: Arc<PersistentMediaCache>,
        ctx: Context,
        rt: Arc<Runtime>,
        client: Client,
        favorite_playlists: Arc<Mutex<HashSet<Uuid>>>,
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
        if let Some(ref data) = cached_playlists {
            fonts::scan_playlists(data);
        }

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
            cached_playlists.as_ref().is_none_or(|d| d.len() > 20),
        ));
        let loading_more = Arc::new(Mutex::new(false));
        let selected_playlist_id = Arc::new(Mutex::new(None));
        let track_search = Arc::new(Mutex::new(String::new()));
        let show_more_menu = Arc::new(Mutex::new(false));
        let more_menu_pos = Arc::new(Mutex::new(None));

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
                    fonts::scan_playlists(&data);
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
            selected_playlist_id,
            track_search,
            show_more_menu,
            more_menu_pos,
            favorite_playlists,
            last_scroll_y: Arc::new(Mutex::new(0.0)),
        }
    }

    /// Returns whether the given playlist is currently favorited.
    pub fn is_playlist_favorite(&self, id: &Uuid) -> bool {
        self.favorite_playlists.blocking_lock().contains(id)
    }

    /// Toggles the favorite state of a playlist: calls the server, then updates the
    /// shared set and requests a repaint so the ♡ button reflects the new state.
    pub fn toggle_playlist_favorite(&self, id: Uuid) {
        let favs = self.favorite_playlists.clone();
        let ctx = self.ctx.clone();
        let songs = self.songs.clone();
        spawn(async move {
            match songs.toggle_playlist_favorite(id).await {
                Ok(is_favorite) => {
                    if is_favorite {
                        favs.lock().await.insert(id);
                    } else {
                        favs.lock().await.remove(&id);
                    }
                }
                Err(e) => {
                    debug_log!("❌ Failed to toggle playlist favorite: {}", e);
                }
            }
            ctx.request_repaint();
        });
    }

    pub async fn clear(&self) {
        *self.playlists.lock().await = LoadingState::Loading;
        *self.all_playlists.lock().await = None;
        *self.selected_playlist.lock().await = None;
        *self.selected_playlist_id.lock().await = None;
        *self.track_search.lock().await = String::new();
        *self.has_more.lock().await = true;
        *self.loading_more.lock().await = false;
        *self.show_more_menu.lock().await = false;
        *self.more_menu_pos.lock().await = None;
        debug_log!("🧹 [Playlist Activity] Resetting memory tables back to pristine state.");
    }

    /// Frees the playlist data (the loaded playlist lists, all-playlists cache,
    /// selected playlist, the `playlist_details` DashMap, and the search/menu
    /// state) so the `Vec<Playlist>`/`Vec<SongDTO>` payloads are dropped when the
    /// user navigates away. Uses `blocking_lock()` so it can be called from the
    /// UI thread. The shared `favorite_playlists` set is kept.
    ///
    /// Also releases every cover-art texture egui registered for the rendered
    /// playlists. Scrolling a large playlist list registers a `bytes://` texture
    /// per cover in egui's texture store; clearing the playlist data alone does
    /// not drop those textures, so we free them explicitly via `ctx.forget_image`.
    pub fn clear_sync(&self, ctx: &Context) {
        let all = self.all_playlists.blocking_lock();
        if let Some(playlists) = &*all {
            for playlist in playlists {
                if let Some(art) = &playlist.media
                    && let Some(uri) = cover_art_uri(&self.cache, art) {
                        ctx.forget_image(&uri);
                    }
                if let Some(mosaic) = &playlist.mosaic_media {
                    for art in mosaic {
                        if let Some(uri) = cover_art_uri(&self.cache, art) {
                            ctx.forget_image(&uri);
                        }
                    }
                }
            }
        }
        drop(all);

        *self.playlists.blocking_lock() = LoadingState::Loading;
        *self.all_playlists.blocking_lock() = None;
        *self.selected_playlist.blocking_lock() = None;
        *self.selected_playlist_id.blocking_lock() = None;
        *self.track_search.blocking_lock() = String::new();
        *self.has_more.blocking_lock() = true;
        *self.loading_more.blocking_lock() = false;
        *self.show_more_menu.blocking_lock() = false;
        *self.more_menu_pos.blocking_lock() = None;
        *self.last_scroll_y.blocking_lock() = 0.0;
        self.playlist_details.clear();
        debug_log!("🧹 [Playlist Activity] Cleared memory and cover textures on navigation away.");
    }

    /// Re-fetches the playlist list from the server, repopulating the loaded
    /// playlists after `clear_sync()`. Mirrors the fetch performed in `new()`.
    pub fn refetch(&self) {
        let cache_file = if self.is_personal {
            "my_playlists.ron"
        } else {
            "playlists.ron"
        };
        let cache_path = playlist_cache_dir().join(cache_file);
        let p = self.playlists.clone();
        let ap = self.all_playlists.clone();
        let hm = self.has_more.clone();
        let songs = self.songs.clone();
        let ctx = self.ctx.clone();
        let is_personal = self.is_personal;
        spawn(async move {
            let result = if is_personal {
                songs.get_user_playlists().await
            } else {
                songs.get_public_playlists(None, false, 0, 500).await
            };
            match result {
                Ok(data) => {
                    fonts::scan_playlists(&data);
                    *ap.lock().await = Some(data.clone());
                    let initial: Vec<_> = data.iter().take(20).cloned().collect();
                    *p.lock().await = LoadingState::Loaded(initial);
                    *hm.lock().await = data.len() > 20;
                    if let Some(parent) = cache_path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let _ = write(
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
            ctx.request_repaint();
        });
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

            fonts::scan_playlists(&all);

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

    /// Renders a full playlist detail page: a header (cover art, title, creator,
    /// stats, action buttons) followed by a searchable "Tracks" table.
    ///
    /// Returns the action chosen in the "more" (kebab) popup, if any, so the
    /// caller can wire it to the corresponding playlist operation.
    pub fn render_full_view(
        &self,
        ui: &mut Ui,
        theme: &ThemeManager,
        playlist: &Playlist,
        detail: &PlaylistDetail,
        play_playlist: &mut impl FnMut(&PlaylistDetail),
        shuffle_playlist: &mut impl FnMut(&PlaylistDetail),
        play_song: &mut impl FnMut(Vec<SongDTO>, Uuid),
        is_personal: bool,
    ) -> Option<MoreMenuAction> {
        // ---- Header ----
        Frame::new()
            .fill(theme.background_elevated)
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(200.0);
                        render_playlist_art_advanced(
                            ui,
                            &self.cache,
                            &self.ctx,
                            &self.rt,
                            &self.client,
                            playlist,
                            &detail.songs,
                            Vec2::new(200.0, 200.0),
                        );
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("PRESENT PLAYLIST")
                                .color(theme.text_muted)
                                .size(12.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(detail.name.to_string())
                                .color(theme.text)
                                .size(28.0)
                                .strong(),
                        );

                        let total_dur: u64 =
                            detail.songs.iter().map(|s| s.duration.unwrap_or(0)).sum();
                        let stats = format!(
                            "Created by {} · {} songs · {}",
                            playlist.creator,
                            detail.songs.len(),
                            format_total_duration(total_dur)
                        );
                        ui.label(RichText::new(stats).color(theme.text_secondary).size(14.0));

                        if let Some(updated) =
                            playlist.updated_at.as_deref().or(playlist.created_at.as_deref())
                        {
                            ui.label(
                                RichText::new(format!(
                                    "Last updated {}",
                                    format_relative_date(updated)
                                ))
                                .color(theme.text_secondary)
                                .size(14.0),
                            );
                        }

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let play_btn = ui.add(
                                Button::new(RichText::new("▶ PLAY").color(theme.text))
                                    .fill(theme.primary)
                                    .min_size(Vec2::new(90.0, 36.0)),
                            );
                            if play_btn.clicked() {
                                play_playlist(detail);
                            }
                            // Shuffle: plays the playlist in random order starting
                            // from a random song.
                            let shuffle_resp = ui.add_sized(
                                [36.0, 36.0],
                                Button::image(
                                    Image::new(include_image!("../../assets/shuffle.svg"))
                                        .fit_to_exact_size(Vec2::new(18.0, 18.0))
                                        .tint(theme.text),
                                )
                                .fill(theme.background_hover),
                            );
                            let shuffle_resp = shuffle_resp.on_hover_text("Shuffle");
                            if shuffle_resp.clicked() {
                                shuffle_playlist(detail);
                            }

                            // Favorite: toggles this playlist's favorite status.
                            let is_fav = self.is_playlist_favorite(&playlist.id);
                            let fav_icon = if is_fav {
                                include_image!("../../assets/favorite.svg")
                            } else {
                                include_image!("../../assets/favorite-empty.svg")
                            };
                            let fav_resp = ui
                                .add_sized(
                                    [36.0, 36.0],
                                    Button::image(
                                        Image::new(fav_icon)
                                            .fit_to_exact_size(Vec2::new(18.0, 18.0))
                                            .tint(if is_fav {
                                                theme.accent_light
                                            } else {
                                                theme.text
                                            }),
                                    )
                                    .fill(theme.background_hover),
                                )
                                .on_hover_text("Favorite");
                            if fav_resp.clicked() {
                                self.toggle_playlist_favorite(playlist.id);
                            }

                            // More (kebab) menu: toggles the popup below the button.
                            let more_resp = ui.add_sized(
                                [36.0, 36.0],
                                Button::image(
                                    Image::new(include_image!("../../assets/more.svg"))
                                        .fit_to_exact_size(Vec2::new(18.0, 18.0))
                                        .tint(theme.text),
                                )
                                .fill(theme.background_hover),
                            );
                            let more_resp = more_resp.on_hover_text("More options");
                            if more_resp.clicked() {
                                let show = !*self.show_more_menu.blocking_lock();
                                *self.show_more_menu.blocking_lock() = show;
                                if show {
                                    *self.more_menu_pos.blocking_lock() =
                                        Some(more_resp.rect.left_bottom() + Vec2::new(0.0, 4.0));
                                }
                                ui.ctx().request_repaint();
                            }
                        });
                    });
                });
            });

        // ---- More (kebab) popup ----
        let mut action: Option<MoreMenuAction> = None;
        if *self.show_more_menu.blocking_lock() {
            let pos = self.more_menu_pos.blocking_lock().unwrap_or_default();
            // Used only by the commented-out "Add to Playlist" block.
            // let all_pls = self.all_playlists.blocking_lock().clone();
            Area::new(Id::new("playlist_more_menu"))
                .fixed_pos(pos)
                .order(Order::Foreground)
                .show(&self.ctx, |ui| {
                    Frame::window(ui.style()).show(ui, |ui| {
                        ui.set_min_width(240.0);

                        // The options below are not wired to the backend yet, so they
                        // are commented out for this commit.
                        /*
                        ui.label(
                            RichText::new("Download")
                                .strong()
                                .color(theme.text_secondary)
                                .size(12.0),
                        );
                        if ui
                            .button(format!("Download all ({} songs)", detail.songs.len()))
                            .clicked()
                        {
                            action = Some(MoreMenuAction::DownloadAll);
                        }
                        if ui.button("Download Selected").clicked() {
                            action = Some(MoreMenuAction::DownloadSelected);
                        }
                        ui.separator();

                        if ui
                            .button(format!(
                                "Save all Offline ({} songs)",
                                detail.songs.len()
                            ))
                            .clicked()
                        {
                            action = Some(MoreMenuAction::SaveOffline);
                        }
                        ui.separator();

                        if is_personal {
                            ui.label(
                                RichText::new("Add to Playlist")
                                    .strong()
                                    .color(theme.text_secondary)
                                    .size(12.0),
                            );
                            if ui.button("Create new Playlist").clicked() {
                                action = Some(MoreMenuAction::CreateNewPlaylist);
                            }
                            if let Some(pls) = &all_pls {
                                for pl in pls {
                                    if pl.id != playlist.id
                                        && ui.button(pl.name.to_string()).clicked()
                                    {
                                        action = Some(MoreMenuAction::AddToPlaylist(pl.id));
                                    }
                                }
                            }
                            ui.separator();
                        }

                        if is_personal {
                            if ui.button("Upload Your Songs").clicked() {
                                action = Some(MoreMenuAction::UploadYourSongs);
                            }
                        }
                        */
                        if ui.button("Copy Link to Playlist").clicked() {
                            action = Some(MoreMenuAction::CopyLink);
                        }
                        /*
                        if is_personal {
                            if ui.button("Edit Playlist").clicked() {
                                action = Some(MoreMenuAction::EditPlaylist);
                            }
                            if ui.button("Delete Playlist").clicked() {
                                action = Some(MoreMenuAction::DeletePlaylist);
                            }
                        }
                        */
                    });
                });
            if action.is_some() {
                *self.show_more_menu.blocking_lock() = false;
            }
        }

        ui.add_space(16.0);

        // ---- New Playlist / Add Songs (personal playlists only) ----
        if is_personal {
            ui.horizontal(|ui| {
                ui.add(
                    Button::new(RichText::new("+ New Playlist").color(theme.text))
                        .fill(theme.primary),
                )
                .on_hover_text("Create a new playlist");
                ui.button("+ Add Songs").on_hover_text("Add songs to this playlist");
            });
            ui.add_space(12.0);
        }

        // ---- Search in playlist ----
        let mut search = self.track_search.blocking_lock().clone();
        fonts::ensure_str(&search);
        ui.add_sized(
            [300.0, 24.0],
            TextEdit::singleline(&mut search).hint_text("Search in playlist"),
        );
        *self.track_search.blocking_lock() = search.clone();

        ui.add_space(12.0);

        // ---- Tracks heading + count ----
        ui.horizontal(|ui| {
            ui.label(RichText::new("Tracks").color(theme.text).size(20.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} songs", detail.songs.len()))
                        .color(theme.text_secondary)
                        .size(14.0),
                );
            });
        });
        ui.add_space(8.0);

        // ---- Filter songs by the in-playlist search ----
        let q = search.to_lowercase();
        let filtered: Vec<&SongDTO> = if q.is_empty() {
            detail.songs.iter().collect()
        } else {
            detail
                .songs
                .iter()
                .filter(|s| {
                    s.title.to_lowercase().contains(&q)
                        || s.original_artists
                            .iter()
                            .any(|a| a.to_lowercase().contains(&q))
                })
                .collect()
        };

        let songs_list = detail.songs.clone();

        // ---- Tracks table ----
        TableBuilder::new(ui)
            .id_salt("full-view-tracks")
            .column(Column::exact(30.0))
            .column(Column::remainder())
            .column(Column::exact(80.0))
            .column(Column::exact(100.0))
            .column(Column::exact(60.0))
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.label("#");
                });
                header.col(|ui| {
                    ui.label("Title");
                });
                header.col(|ui| {
                    ui.label("Play Count");
                });
                header.col(|ui| {
                    ui.label("Date Added");
                });
                header.col(|ui| {
                    ui.label("Duration");
                });
            })
            .body(|body| {
                body.rows(20.0, filtered.len(), |mut row| {
                    let song = filtered[row.index()];
                    let idx = row.index();

                    row.col(|ui| {
                        ui.label(format!("{}", idx + 1));
                    });
                    row.col(|ui| {
                        let title =
                            format!("{} - {}", song.original_artists.join(" & "), song.title);
                        if ui.selectable_label(false, title).clicked() {
                            play_song(songs_list.clone(), song.id);
                        }
                    });
                    row.col(|ui| {
                        ui.label(
                            song.play_count
                                .map(format_play_count)
                                .unwrap_or_default(),
                        );
                    });
                    row.col(|ui| {
                        ui.label(
                            song.stream_date
                                .as_ref()
                                .map(|d| format_added_date(d))
                                .unwrap_or_default(),
                        );
                    });
                    row.col(|ui| {
                        render_duration_label(ui, song.duration);
                    });
                });
            });

        action
    }

    pub fn render(
        &self,
        ui: &mut Ui,
        playlist_search: &mut Option<&mut String>,
        current_sort: &mut SortOption,
        current_sort_desc: &mut bool,
        mut play_playlist: impl FnMut(&PlaylistDetail),
        shuffle_playlist: impl FnMut(&PlaylistDetail),
        play_song: impl FnMut(Vec<SongDTO>, Uuid) + 'static,
        is_personal: bool,
        _creator_name: Option<String>,
    ) {
        let play_song = RefCell::new(play_song);
        let shuffle_playlist = RefCell::new(shuffle_playlist);
        ui.vertical(|ui| {
            // Full playlist detail view when a playlist is selected.
            let selected = *self.selected_playlist_id.blocking_lock();
            if let Some(id) = selected {
                let playlist = self
                    .all_playlists
                    .blocking_lock()
                    .as_ref()
                    .and_then(|list| list.iter().find(|p| p.id == id))
                    .cloned();
                if let Some(playlist) = playlist {
                    spawn_fetch_playlist_details(
                        &self.rt,
                        Some(&self.ctx),
                        &self.songs,
                        &self.playlist_details,
                        id,
                        playlist.name.clone(),
                    );
                    let theme = ThemeManager::new(Config::read().unwrap_or_default().theme.as_theme());
                    if let Some(detail_state) = self.playlist_details.get(&id) {
                        match &*detail_state {
                            LoadingState::Loaded(detail) => {
                                ui.horizontal(|ui| {
                                    if ui.button("← Back").clicked() {
                                        *self.selected_playlist_id.blocking_lock() = None;
                                    }
                                });
                                ui.add_space(8.0);
                                let mut action: Option<MoreMenuAction> = None;
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        action = self.render_full_view(
                                            ui,
                                            &theme,
                                            &playlist,
                                            detail,
                                            &mut play_playlist,
                                            &mut *shuffle_playlist.borrow_mut(),
                                            &mut *play_song.borrow_mut(),
                                            is_personal,
                                        );
                                    });
                                if let Some(action) = action {
                                    match action {
                                        MoreMenuAction::CopyLink => {
                                            let url = format!(
                                                "https://neurokaraoke.com/playlist/{}",
                                                playlist.id
                                            );
                                            debug_log!("Copied playlist link: {}", url);
                                            self.ctx.copy_text(url);
                                        }
                                    }
                                }
                                return;
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
            }

            if let Some(search) = playlist_search {
                fonts::ensure_str(search);
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
                            if let Ok(mut all_opt) = self.all_playlists.try_lock()
                                && let Some(ref mut all) = *all_opt {
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
                                    if let Ok(mut playlists_guard) = self.playlists.try_lock()
                                        && let LoadingState::Loaded(list) = &mut *playlists_guard {
                                            let current_len = list.len().max(20);
                                            *list = all.iter().take(current_len).cloned().collect();
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
                                            criteria.creator.as_ref().is_none_or(|q| {
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

                            let viewport_height = ui.available_height();
                            let len = sorted_playlists.len();
                            let box_height = 260.0f32;
                            let offset_y = *self.last_scroll_y.blocking_lock();
                            // Clamp start to len: a stale deep scroll offset
                            // combined with a shrunk list (search filter, sort,
                            // load-more reset) would otherwise panic when
                            // slicing with start > len.
                            let (start, end) = visible_range(offset_y, viewport_height, box_height, len);
                            let scroll_output = ScrollArea::vertical().show(ui, |ui| {
                                ui.add_space(start as f32 * box_height);
                                for playlist in sorted_playlists[start..end].iter() {
                                    spawn_fetch_playlist_details(
                                        &self.rt,
                                        Some(&self.ctx),
                                        &self.songs,
                                        &self.playlist_details,
                                        playlist.id,
                                        playlist.name.clone(),
                                    );

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
                                                        let cover_size = Vec2::new(150.0, 150.0);
                                                        let top = ui.cursor().min;
                                                        render_playlist_art_advanced(
                                                            ui,
                                                            &self.cache,
                                                            &self.ctx,
                                                            &self.rt,
                                                            &self.client,
                                                            playlist,
                                                            &detail.songs,
                                                            cover_size,
                                                        );
                                                        let rect =
                                                            Rect::from_min_max(top, top + cover_size);
                                                        let resp = ui.interact(
                                                            rect,
                                                            ui.id().with(format!(
                                                                "cover_{}",
                                                                playlist.id
                                                            )),
                                                            Sense::click(),
                                                        );
                                                        if resp.clicked() {
                                                            *self.selected_playlist_id
                                                                .blocking_lock() =
                                                                Some(playlist.id);
                                                        }
                                                    },
                                                );

                                                ui.horizontal(|ui| {
                                                    if ui.button("Play Playlist").clicked() {
                                                        play_playlist(detail);
                                                    }
                                                    if ui.button("View Playlist").clicked() {
                                                        *self.selected_playlist_id.blocking_lock() =
                                                            Some(playlist.id);
                                                    }
                                                });
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
                                ui.add_space((len - end) as f32 * box_height);

                                // Load more button at the bottom of playlists
                                if playlist_search.is_none()
                                    || playlist_search.as_ref().is_none_or(|s| s.is_empty())
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
                            *self.last_scroll_y.blocking_lock() = offset_y as f64;
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

/// Formats an ISO-8601 timestamp (e.g. `2026-09-30T17:50:57.9856479`) as a
/// human-friendly relative string such as "1 month ago".
pub fn format_relative_date(iso: &str) -> String {
    let dt = match chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f") {
        Ok(dt) => dt,
        Err(_) => return iso.to_string(),
    };
    let now = chrono::Utc::now().naive_utc();
    let dur = now.signed_duration_since(dt);
    let days = dur.num_days();
    if days < 1 {
        let hours = dur.num_hours();
        if hours < 1 {
            let mins = dur.num_minutes();
            if mins < 1 {
                "just now".to_string()
            } else {
                format!("{} minutes ago", mins)
            }
        } else {
            format!("{} hours ago", hours)
        }
    } else if days < 7 {
        format!("{} days ago", days)
    } else if days < 30 {
        format!("{} weeks ago", days / 7)
    } else if days < 365 {
        format!("{} months ago", days / 30)
    } else {
        format!("{} years ago", days / 365)
    }
}

#[cfg(test)]
mod tests {
    use super::format_relative_date;

    fn iso_minutes_ago(mins: i64) -> String {
        let now = chrono::Utc::now().naive_utc();
        (now - chrono::Duration::minutes(mins))
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string()
    }

    #[test]
    fn test_format_relative_date_minutes() {
        assert_eq!(format_relative_date(&iso_minutes_ago(30)), "30 minutes ago");
    }

    #[test]
    fn test_format_relative_date_hours() {
        assert_eq!(format_relative_date(&iso_minutes_ago(120)), "2 hours ago");
    }

    #[test]
    fn test_format_relative_date_months() {
        assert_eq!(format_relative_date(&iso_minutes_ago(45 * 24 * 60)), "1 months ago");
    }

    #[test]
    fn test_format_relative_date_invalid_passthrough() {
        assert_eq!(format_relative_date("not-a-date"), "not-a-date");
    }
}
