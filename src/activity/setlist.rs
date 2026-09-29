use crate::activity::*;
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail};
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::{PersistentMediaCache, cache_dir};
use dashmap::DashMap;
use eframe::egui::{Context, ScrollArea, Ui};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct SetlistActivity {
    pub setlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub setlist_details: Arc<DashMap<Uuid, LoadingState<PlaylistDetail>>>,
    pub selected_setlist: Arc<Mutex<Option<PlaylistDetail>>>,
    pub songs: LazySongDatabase,
    pub current_year: Arc<Mutex<u32>>,
    pub cache: Arc<PersistentMediaCache>,
    pub ctx: Context,
    pub rt: Arc<Runtime>,
    pub client: Client,
}

impl SetlistActivity {
    fn get_cache_path(year: u32) -> PathBuf {
        cache_dir().join(format!("setlists_{}.ron", year))
    }

    pub fn new(
        songs: LazySongDatabase,
        cache: Arc<PersistentMediaCache>,
        ctx: Context,
        rt: Arc<Runtime>,
        client: Client,
    ) -> Self {
        let setlists = Arc::new(Mutex::new(LoadingState::Loading));
        let current_year = Arc::new(Mutex::new(0));

        let activity = Self {
            setlists: setlists.clone(),
            setlist_details: Arc::new(DashMap::new()),
            selected_setlist: Arc::new(Mutex::new(None)),
            songs: songs.clone(),
            current_year: current_year.clone(),
            cache,
            ctx,
            rt,
            client,
        };

        activity.load_setlists(0);

        activity
    }

    pub fn select_setlist(&self, id: Uuid) {
        let details = self.setlist_details.clone();
        let selected = self.selected_setlist.clone();
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

    fn load_setlists(&self, year: u32) {
        let s = self.setlists.clone();
        let details = self.setlist_details.clone();
        let songs = self.songs.clone();
        let rt = self.rt.clone();

        tokio::spawn(async move {
            let cache_path = Self::get_cache_path(year);

            // Try loading from cache first (optimistic load)
            if let Ok(data) = tokio::fs::read(&cache_path).await {
                if let Ok(setlists) = ron::de::from_bytes::<Vec<Playlist>>(&data) {
                    *s.lock().await = LoadingState::Loaded(setlists);
                }
            }

            // Always fetch fresh setlists from network
            match songs.get_official_setlists(year).await {
                Ok(data) => {
                    debug_log!(
                        "Fetched fresh setlists for year {}: {} items",
                        year,
                        data.len()
                    );

                    // Retain only details that exist in the newly fetched setlists
                    let setlist_ids: std::collections::HashSet<Uuid> =
                        data.iter().map(|p| p.id).collect();

                    let before_count = details.len();
                    details.retain(|id, _| setlist_ids.contains(id));
                    let after_count = details.len();

                    debug_log!(
                        "Retained {} of {} playlist details",
                        after_count,
                        before_count
                    );

                    for setlist in &data {
                        // Only fetch if not already present
                        if !details.contains_key(&setlist.id) {
                            details.insert(setlist.id, LoadingState::Loading);

                            // Spawn fetch for detail
                            let s_id = setlist.id;
                            let s_songs = songs.clone();
                            let s_details = details.clone();
                            rt.spawn(async move {
                                match s_songs.get_playlist_details(s_id).await {
                                    Ok(detail) => {
                                        s_details.insert(s_id, LoadingState::Loaded(detail));
                                    }
                                    Err(err) => {
                                        s_details.insert(s_id, LoadingState::Failed(Arc::new(err)));
                                    }
                                }
                            });
                        }
                    }
                    *s.lock().await = LoadingState::Loaded(data.clone());
                    let _ = tokio::fs::write(
                        cache_path,
                        ron::ser::to_string_pretty(&data, Default::default()).unwrap(),
                    )
                    .await;
                }
                Err(err) => {
                    let mut s_lock = s.lock().await;
                    if matches!(*s_lock, LoadingState::Loading) {
                        *s_lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }
        });
    }

    pub fn set_year(&self, year: u32) {
        *self.current_year.blocking_lock() = year;
        self.load_setlists(year);
    }

    pub fn render(
        &self,
        ui: &mut Ui,
        theme: &ThemeManager,
        mut play_playlist: impl FnMut(&PlaylistDetail),
        play_song: impl FnMut(Vec<SongDTO>, Uuid) + 'static,
    ) {
        let play_song = std::cell::RefCell::new(play_song);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Official Setlists:");
                let current_year = *self.current_year.blocking_lock();
                for &year in &[2026, 2025, 2024, 2023] {
                    if ui
                        .selectable_label(current_year == year, year.to_string())
                        .clicked()
                    {
                        self.set_year(year);
                    }
                }
            });

            let setlists_lock = self.setlists.try_lock();
            match setlists_lock {
                Ok(setlists) => match &*setlists {
                    LoadingState::Loaded(setlists) => {
                        ScrollArea::vertical().show(ui, |ui| {
                            for setlist in setlists {
                                if let Some(detail_state) = self.setlist_details.get(&setlist.id) {
                                    match &*detail_state {
                                        LoadingState::Loaded(detail) => {
                                            render_playlist_box(
                                                ui,
                                                theme,
                                                &format!("setlist_box_{}", setlist.id),
                                                &detail.name,
                                                "",
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
                                                        setlist,
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
                                            ui.label(format!("Loading {}...", setlist.name));
                                        }
                                        LoadingState::Failed(err) => {
                                            ui.label(format!(
                                                "Error loading setlist {}: {}",
                                                setlist.name, err
                                            ));
                                        }
                                    }
                                }
                            }
                        });
                    }
                    LoadingState::Loading => {
                        ui.label("Loading...");
                    }
                    LoadingState::Failed(err) => {
                        ui.label(format!("Error loading setlists: {}", err));
                    }
                },
                Err(_) => {
                    ui.label("Loading...");
                }
            }
        });
    }
}
