use crate::activity::*;
use crate::api::{LazySongDatabase, LoadingState, Playlist, PlaylistDetail};
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::{PersistentMediaCache, cache_dir};
use crate::utilities::util::spawn_fetch_playlist_details;
use chrono::{Datelike, Utc};
use dashmap::DashMap;
use eframe::egui::{Context, ScrollArea, Ui};
use ron::de::from_bytes;
use ron::ser::to_string_pretty;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{read, write};
use tokio::spawn;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct SetlistActivity {
    pub setlists: Arc<Mutex<LoadingState<Vec<Playlist>>>>,
    pub setlist_details: Arc<DashMap<Uuid, LoadingState<PlaylistDetail>>>,
    pub _selected_setlist: Arc<Mutex<Option<PlaylistDetail>>>,
    pub songs: LazySongDatabase,
    pub current_year: Arc<Mutex<u32>>,
    pub available_years: Arc<Mutex<Vec<u32>>>,
    pub last_total_count: Arc<Mutex<Option<u64>>>,
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
        let year = Utc::now().year() as u32;
        let current_year = Arc::new(Mutex::new(year));
        let available_years = Arc::new(Mutex::new(vec![]));
        let last_total_count = Arc::new(Mutex::new(None));

        let activity = Self {
            setlists: setlists.clone(),
            setlist_details: Arc::new(DashMap::new()),
            _selected_setlist: Arc::new(Mutex::new(None)),
            songs: songs.clone(),
            current_year,
            available_years,
            last_total_count,
            cache,
            ctx,
            rt,
            client,
        };

        activity.load_setlists(year);

        activity
    }

    pub async fn check_and_update_stats(&self, remote_total_count: u64) {
        let mut last_count = self.last_total_count.lock().await;
        if last_count.map_or(true, |c| c != remote_total_count) {
            debug_log!(
                "Setlist count mismatch: remote totalCount={}, previous={:?}. Retrieving setlists again...",
                remote_total_count,
                *last_count
            );
            *last_count = Some(remote_total_count);
            let year = *self.current_year.lock().await;
            self.load_setlists(year);
        }
    }

    fn load_setlists(&self, year: u32) {
        let s = self.setlists.clone();
        let details = self.setlist_details.clone();
        let available_years = self.available_years.clone();
        let last_total_count = self.last_total_count.clone();
        let songs = self.songs.clone();
        let rt = self.rt.clone();
        let ctx = self.ctx.clone();

        spawn(async move {
            let cache_path = Self::get_cache_path(year);

            // Try loading from cache first
            let mut has_cached = false;
            if let Ok(data) = read(&cache_path).await {
                if let Ok(setlists) = from_bytes::<Vec<Playlist>>(&data) {
                    for setlist in &setlists {
                        spawn_fetch_playlist_details(
                            &rt,
                            Some(&ctx),
                            &songs,
                            &details,
                            setlist.id,
                            setlist.name.clone(),
                        );
                    }
                    *s.lock().await = LoadingState::Loaded(setlists);
                    has_cached = true;
                    ctx.request_repaint();
                }
            }

            // Poll setlist stats before updating cached setlists
            match songs.get_setlist_stats().await {
                Ok(stats) => {
                    debug_log!(
                        "Fetched setlist stats: totalCount={}, years={:?}",
                        stats.total_count,
                        stats.years
                    );
                    *last_total_count.lock().await = Some(stats.total_count);
                    if !stats.years.is_empty() {
                        *available_years.lock().await = stats.years.clone();
                    }
                }
                Err(err) => {
                    debug_log!("Failed to fetch setlist stats: {}", err);
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
                    let setlist_ids: HashSet<Uuid> = data.iter().map(|p| p.id).collect();

                    let before_count = details.len();
                    details.retain(|id, _| setlist_ids.contains(id));
                    let after_count = details.len();

                    debug_log!(
                        "Retained {} of {} playlist details",
                        after_count,
                        before_count
                    );

                    for setlist in &data {
                        spawn_fetch_playlist_details(
                            &rt,
                            Some(&ctx),
                            &songs,
                            &details,
                            setlist.id,
                            setlist.name.clone(),
                        );
                    }
                    *s.lock().await = LoadingState::Loaded(data.clone());
                    let _ = write(
                        cache_path,
                        to_string_pretty(&data, Default::default()).unwrap(),
                    )
                    .await;
                }
                Err(err) => {
                    let mut s_lock = s.lock().await;
                    if !has_cached && matches!(*s_lock, LoadingState::Loading) {
                        *s_lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }
            ctx.request_repaint();
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
        let play_song = RefCell::new(play_song);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Selected year:");
                let current_year = *self.current_year.blocking_lock();
                let available_years = self.available_years.blocking_lock();
                let years = if available_years.is_empty() {
                    vec![2026, 2025, 2024, 2023]
                } else {
                    available_years.clone()
                };
                for &year in &years {
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
