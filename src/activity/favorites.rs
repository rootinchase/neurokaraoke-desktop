use crate::activity::profile::ProfileActivity;
use crate::activity::{render_mosaic, render_playlist_art, render_playlist_box};
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail, SongDTO};
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::{PersistentMediaCache, cache_dir};
use crate::utilities::util::get_playlist_details_cached;
use dashmap::DashMap;
use eframe::egui::{Context, ScrollArea, Ui, Vec2};
use reqwest::Client;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use ron::de::from_bytes;
use ron::ser::to_string_pretty;
use tokio::fs::{read, write};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

pub struct FavoritesActivity {
    pub ctx: Context,
    pub cache: Arc<PersistentMediaCache>,
    pub rt: Arc<Runtime>,
    pub client: Client,
    pub db: LazySongDatabase,
    pub playlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub playlist_details: Arc<DashMap<Uuid, LoadingState<PlaylistDetail>>>,
    pub selected_playlist: Arc<Mutex<Option<PlaylistDetail>>>,
    pub is_fetching: Arc<AtomicBool>,
    pub favorite_songs: Arc<RwLock<HashSet<Uuid>>>,
}

impl FavoritesActivity {
    pub fn new(
        ctx: Context,
        cache: Arc<PersistentMediaCache>,
        rt: Arc<Runtime>,
        client: Client,
        songs: LazySongDatabase,
    ) -> Self {
        Self {
            ctx,
            cache,
            rt,
            client,
            db: songs,
            playlists: Arc::new(Mutex::new(LoadingState::Loading)),
            songs: Arc::new(Mutex::new(LoadingState::Loading)),
            playlist_details: Arc::new(DashMap::new()),
            selected_playlist: Arc::new(Mutex::new(None)),
            is_fetching: Arc::new(AtomicBool::new(false)),
            favorite_songs: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn select_playlist(&self, id: Uuid) {
        let details = self.playlist_details.clone();
        let selected = self.selected_playlist.clone();
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            if let Some(detail_state) = details.get(&id) {
                if let LoadingState::Loaded(detail) = &*detail_state {
                    *selected.lock().await = Some(detail.clone());
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn is_favorite(&self, id: &Uuid) -> bool {
        self.favorite_songs.blocking_read().contains(id)
    }

    pub fn toggle_favorite(&self, id: Uuid, db: &LazySongDatabase) {
        let favs = self.favorite_songs.clone();
        let db = db.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            let is_favorite = favs.read().await.contains(&id);
            if is_favorite {
                if db.remove_from_favorites(id).await.is_ok() {
                    favs.write().await.remove(&id);
                }
            } else {
                if db.add_to_favorites(id).await.is_ok() {
                    favs.write().await.insert(id);
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn fetch_favorites(&self) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }

        let p = self.playlists.clone();
        let s = self.songs.clone();
        let ctx = self.ctx.clone();
        let db = self.db.clone();
        let is_fetching = self.is_fetching.clone();
        let details = self.playlist_details.clone();
        let rt = self.rt.clone();

        *p.blocking_lock() = LoadingState::Loading;
        *s.blocking_lock() = LoadingState::Loading;

        tokio::spawn(async move {
            let playlists_cache_path = cache_dir().join("favorites_playlists.ron");
            let songs_cache_path = cache_dir().join("favorites_songs.ron");

            let mut has_cached_playlists = false;
            if let Ok(data) = read(&playlists_cache_path).await {
                if let Ok(playlists) = from_bytes::<Vec<Playlist>>(&data) {
                    for playlist in &playlists {
                        if !details.contains_key(&playlist.id) {
                            details.insert(playlist.id, LoadingState::Loading);
                            let p_id = playlist.id;
                            let p_name = playlist.name.clone();
                            let p_songs = db.clone();
                            let p_details = details.clone();
                            rt.spawn(async move {
                                match get_playlist_details_cached(
                                    p_id, &p_songs,
                                )
                                .await
                                {
                                    Ok(detail) => {
                                        p_details.insert(p_id, LoadingState::Loaded(detail));
                                    }
                                    Err(_) => {
                                        p_details.insert(
                                            p_id,
                                            LoadingState::Loaded(PlaylistDetail {
                                                name: p_name,
                                                songs: vec![],
                                            }),
                                        );
                                    }
                                }
                            });
                        }
                    }
                    *p.lock().await = LoadingState::Loaded(playlists);
                    has_cached_playlists = true;
                }
            }

            let mut has_cached_songs = false;
            if let Ok(data) = read(&songs_cache_path).await {
                if let Ok(song_list) = from_bytes::<Vec<SongDTO>>(&data) {
                    *s.lock().await = LoadingState::Loaded(song_list);
                    has_cached_songs = true;
                }
            }

            // Fetch playlists
            match db.fetch_favorite_playlists().await {
                Ok(playlists) => {
                    let _ = write(
                        &playlists_cache_path,
                        to_string_pretty(&playlists, Default::default()).unwrap(),
                    )
                    .await;

                    for playlist in &playlists {
                        if !details.contains_key(&playlist.id) {
                            details.insert(playlist.id, LoadingState::Loading);
                            let p_id = playlist.id;
                            let p_name = playlist.name.clone();
                            let p_songs = db.clone();
                            let p_details = details.clone();
                            rt.spawn(async move {
                                match get_playlist_details_cached(
                                    p_id, &p_songs,
                                )
                                .await
                                {
                                    Ok(detail) => {
                                        p_details.insert(p_id, LoadingState::Loaded(detail));
                                    }
                                    Err(_) => {
                                        p_details.insert(
                                            p_id,
                                            LoadingState::Loaded(PlaylistDetail {
                                                name: p_name,
                                                songs: vec![],
                                            }),
                                        );
                                    }
                                }
                            });
                        }
                    }
                    *p.lock().await = LoadingState::Loaded(playlists);
                }
                Err(err) => {
                    if !has_cached_playlists {
                        *p.lock().await = LoadingState::Failed(Arc::new(err));
                    }
                }
            }

            // Fetch songs
            match db.get_favorite_songs().await {
                Ok(song_list) => {
                    let _ = write(
                        &songs_cache_path,
                        to_string_pretty(&song_list, Default::default()).unwrap(),
                    )
                    .await;
                    *s.lock().await = LoadingState::Loaded(song_list);
                }
                Err(err) => {
                    if !has_cached_songs {
                        *s.lock().await = LoadingState::Failed(Arc::new(err));
                    }
                }
            }

            is_fetching.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    pub async fn clear(&self) {
        *self.playlists.lock().await = LoadingState::Loading;
        *self.songs.lock().await = LoadingState::Loading;
        self.playlist_details.clear();
        self.is_fetching.store(false, Ordering::SeqCst);
        self.favorite_songs.write().await.clear();

        debug_log!("🧹 [Favorites Activity] UI data tracks flushed to default baseline states.");
    }

    pub fn render(
        &self,
        ui: &mut Ui,
        theme: &ThemeManager,
        profile_activity: &ProfileActivity,
        mut play_playlist: impl FnMut(&PlaylistDetail),
        play_song: impl FnMut(Vec<SongDTO>, Uuid) + 'static,
    ) {
        let should_fetch = {
            let favorites = self.playlists.blocking_lock();
            matches!(*favorites, LoadingState::Loading)
        };

        if should_fetch {
            self.fetch_favorites();
        }

        let play_song = RefCell::new(play_song);

        ui.vertical(|ui| {
            ui.add_space(10.0);

            let favorites = self.playlists.blocking_lock();
            let favorite_songs = self.songs.blocking_lock();

            match (&*favorites, &*favorite_songs) {
                (LoadingState::Loaded(playlists), LoadingState::Loaded(songs)) => {
                    ScrollArea::vertical().show(ui, |ui| {
                        // Render "Favorite Songs" card box
                        let favorite_songs_detail = PlaylistDetail {
                            name: "Favorite Songs".into(),
                            songs: songs.clone(),
                        };
                        let creator = profile_activity
                            .state
                            .profile_data
                            .as_ref()
                            .map(|p| p.display_name.clone())
                            .unwrap_or_else(|| "You".to_string());

                        render_playlist_box(
                            ui,
                            theme,
                            "favorites_box_favorite_songs",
                            "Favorite Songs",
                            &creator,
                            &favorite_songs_detail,
                            &mut |s_list, song_id| {
                                play_song.borrow_mut()(s_list, song_id);
                            },
                            |ui| {
                                render_mosaic(
                                    ui,
                                    &self.cache,
                                    &self.ctx,
                                    &self.rt,
                                    &self.client,
                                    "favorite_songs_mosaic",
                                    songs,
                                );
                            },
                        );

                        if ui.button("Play Playlist").clicked() {
                            play_playlist(&favorite_songs_detail);
                        }

                        ui.add_space(20.0);

                        // Render each favorite playlist card box
                        for playlist in playlists {
                            if !self.playlist_details.contains_key(&playlist.id) {
                                self.playlist_details
                                    .insert(playlist.id, LoadingState::Loading);
                                let p_id = playlist.id;
                                let p_name = playlist.name.clone();
                                let p_songs = self.db.clone();
                                let p_details = self.playlist_details.clone();
                                let rt = self.rt.clone();
                                let ctx = self.ctx.clone();
                                rt.spawn(async move {
                                    match get_playlist_details_cached(
                                        p_id, &p_songs,
                                    )
                                    .await
                                    {
                                        Ok(detail) => {
                                            p_details.insert(p_id, LoadingState::Loaded(detail));
                                        }
                                        Err(_) => {
                                            p_details.insert(
                                                p_id,
                                                LoadingState::Loaded(PlaylistDetail {
                                                    name: p_name,
                                                    songs: vec![],
                                                }),
                                            );
                                        }
                                    }
                                    ctx.request_repaint();
                                });
                            }

                            if let Some(detail_state) = self.playlist_details.get(&playlist.id) {
                                match &*detail_state {
                                    LoadingState::Loaded(detail) => {
                                        render_playlist_box(
                                            ui,
                                            theme,
                                            &format!("favorites_playlist_box_{}", playlist.id),
                                            &detail.name,
                                            &playlist.creator,
                                            detail,
                                            &mut |s_list, song_id| {
                                                play_song.borrow_mut()(s_list, song_id);
                                            },
                                            |ui| {
                                                render_playlist_art(
                                                    ui,
                                                    &self.cache,
                                                    &self.ctx,
                                                    &self.rt,
                                                    &self.client,
                                                    playlist,
                                                    Vec2::new(150.0, 150.0),
                                                );
                                            },
                                        );

                                        if ui.button("Play Playlist").clicked() {
                                            play_playlist(detail);
                                        }
                                        ui.add_space(20.0);
                                    }
                                    LoadingState::Loading => {
                                        ui.label(format!("Loading {}...", playlist.name));
                                        ui.add_space(10.0);
                                    }
                                    LoadingState::Failed(err) => {
                                        ui.label(format!(
                                            "Error loading playlist {}: {}",
                                            playlist.name, err
                                        ));
                                        ui.add_space(10.0);
                                    }
                                }
                            }
                        }
                    });
                }
                (LoadingState::Loading, _) | (_, LoadingState::Loading) => {
                    ui.label("Loading favorites...");
                }
                (LoadingState::Failed(err), _) | (_, LoadingState::Failed(err)) => {
                    ui.label(format!("Error loading favorites: {}", err));
                }
            }
        });
    }
}
