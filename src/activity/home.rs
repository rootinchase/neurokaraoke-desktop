use crate::api::{LazySongDatabase, LoadingState, SongDTO, TrendingTimes};
use crate::utilities::cache::{self, PersistentMediaCache};
use crate::utilities::persistence::{AppState, load_app_state};
use crate::theme::ThemeManager;
use eframe::egui::{
    Button, Frame, Grid, Image, RichText, Sense, Ui, Vec2, include_image,
};
use egui_extras::{Column, TableBuilder};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::debug_log;

pub struct HomeActivity {
    pub ctx: eframe::egui::Context,
    pub cache: Arc<PersistentMediaCache>,
    pub suggested_songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub trending_songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub setlist_songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub setlist_name: Arc<Mutex<Option<String>>>,
    pub rt: Arc<tokio::runtime::Runtime>,
    pub client: reqwest::Client,
    pub songs: LazySongDatabase,
}

impl HomeActivity {
    pub fn new(
        ctx: eframe::egui::Context,
        cache: Arc<PersistentMediaCache>,
        rt: Arc<tokio::runtime::Runtime>,
        client: reqwest::Client,
        songs: LazySongDatabase,
    ) -> Self {
        Self {
            ctx,
            cache,
            suggested_songs: Arc::new(Mutex::new(LoadingState::Loading)),
            trending_songs: Arc::new(Mutex::new(LoadingState::Loading)),
            setlist_songs: Arc::new(Mutex::new(LoadingState::Loading)),
            setlist_name: Arc::new(Mutex::new(None)),
            rt,
            client,
            songs,
        }
    }

    pub fn fetch_suggested(&self) {
        let songs_state = self.suggested_songs.clone();
        let songs = self.songs.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            eprintln!("HomeActivity: Fetching suggestions...");
            *songs_state.lock().await = LoadingState::Loading;
            match songs.get_suggested(20).await {
                Ok(data) => {
                    eprintln!(
                        "HomeActivity: Successfully fetched {} suggestions",
                        data.len()
                    );
                    *songs_state.lock().await = LoadingState::Loaded(data);
                }
                Err(err) => {
                    eprintln!("HomeActivity: Failed to fetch suggestions: {:?}", err);
                    *songs_state.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn fetch_trending(&self) {
        let songs_state = self.trending_songs.clone();
        let songs = self.songs.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            debug_log!("HomeActivity: Fetching trending...");
            *songs_state.lock().await = LoadingState::Loading;
            match songs.get_trending(TrendingTimes::Week).await {
                Ok(data) => {
                    debug_log!("HomeActivity: Successfully fetched {} trending", data.len());
                    *songs_state.lock().await = LoadingState::Loaded(data);
                }
                Err(err) => {
                    debug_log!("HomeActivity: Failed to fetch trending: {:?}", err);
                    *songs_state.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
            ctx.request_repaint();
        });
    }

    pub fn fetch_recent_setlist(&self) {
        let songs_state = self.setlist_songs.clone();
        let setlist_name = self.setlist_name.clone();
        let songs = self.songs.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            debug_log!("HomeActivity: Fetching setlists...");
            *songs_state.lock().await = LoadingState::Loading;
            match songs.get_official_setlists(2026).await {
                Ok(data) => {
                    if let Some(setlist) = data.first() {
                        debug_log!("HomeActivity: Successfully fetched setlists, fetching details for {}", setlist.id);
                        match songs.get_playlist_details(setlist.id).await {
                            Ok(detail) => {
                                debug_log!("HomeActivity: Successfully fetched {} songs from setlist", detail.songs.len());
                                let name = detail.name.to_string().replace("Setlist", "").trim().to_string();
                                *setlist_name.lock().await = Some(name);
                                *songs_state.lock().await = LoadingState::Loaded(detail.songs);
                            }
                            Err(err) => {
                                debug_log!("HomeActivity: Failed to fetch setlist details: {:?}", err);
                                *songs_state.lock().await = LoadingState::Failed(Arc::new(err));
                            }
                        }
                    } else {
                        *songs_state.lock().await = LoadingState::Loaded(vec![]);
                    }
                }
                Err(err) => {
                    debug_log!("HomeActivity: Failed to fetch setlists: {:?}", err);
                    *songs_state.lock().await = LoadingState::Failed(Arc::new(err));
                }
            }
            ctx.request_repaint();
        });
    }

    fn resolve_and_render_art(&self, ui: &mut Ui, cover_art: &crate::api::Artwork, size: Vec2) {
        crate::activity::resolve_and_render_art(
            ui,
            &self.cache,
            &self.ctx,
            &self.rt,
            &self.client,
            cover_art,
            size,
        );
    }

    fn render_mosaic(&self, ui: &mut Ui, id: &str, songs_list: &[SongDTO]) {
        let size = Vec2::new(150.0, 150.0);
        Grid::new(format!("{} songs", id))
            .spacing(Vec2::new(2.0, 2.0))
            .show(ui, |ui| {
                for i in 0..4 {
                    if let Some(song) = songs_list.get(i) {
                        if let Some(cover) = &song.cover_art {
                            self.resolve_and_render_art(ui, cover, size / 2.0);
                        } else {
                            ui.allocate_exact_size(size / 2.0, Sense::hover());
                        }
                    } else {
                        ui.allocate_exact_size(size / 2.0, Sense::hover());
                    }
                    if i == 1 {
                        ui.end_row();
                    }
                }
            });
    }

    fn render_song_list(
        &self,
        ui: &mut Ui,
        theme: &ThemeManager,
        id: &str,
        title: &str,
        label: &str,
        songs_state: &Mutex<LoadingState<Vec<SongDTO>>>,
        on_play_suggested: &impl Fn(Vec<SongDTO>, Uuid),
    ) {
        let songs_list = songs_state.blocking_lock();

        Frame::new()
            .fill(theme.background_elevated)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        if let LoadingState::Loaded(songs) = &*songs_list {
                            self.render_mosaic(ui, id, songs);
                        } else {
                            ui.allocate_exact_size(Vec2::new(150.0, 150.0), Sense::hover());
                        };
                        ui.heading(title);
                        ui.label(label);
                    });

                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(200.0);

                        let table = TableBuilder::new(ui)
                            .id_salt(format!("{}-songs", id))
                            .column(Column::remainder())
                            .column(Column::exact(60.0))
                            .column(Column::exact(100.0))
                            .column(Column::exact(60.0))
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.label("Song");
                                });
                                header.col(|ui| {
                                    ui.label("Plays");
                                });
                                header.col(|ui| {
                                    ui.label("Date");
                                });
                                header.col(|ui| {
                                    ui.label("Dur");
                                });
                            });

                        match &*songs_list {
                            LoadingState::Loaded(songs_list) => {
                                let songs_list_clone = songs_list.clone();
                                table.body(|body| {
                                    body.rows(20.0, songs_list.len(), |mut row| {
                                        let song = &songs_list[row.index()];
                                        let song_id = song.id;

                                        row.col(|ui| {
                                            if ui
                                                .selectable_label(
                                                    false,
                                                    format!(
                                                        "{} - {}",
                                                        song.original_artists.join(" & "),
                                                        song.title
                                                    ),
                                                )
                                                .clicked()
                                            {
                                                on_play_suggested(
                                                    songs_list_clone.clone(),
                                                    song_id,
                                                );
                                            }
                                        });

                                        row.col(|ui| {
                                            ui.label(
                                                song.play_count
                                                    .map(|c| c.to_string())
                                                    .unwrap_or_default(),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.label(
                                                song.stream_date
                                                    .as_ref()
                                                    .map(|d| d.to_string())
                                                    .unwrap_or_default(),
                                            );
                                        });
                                        row.col(|ui| {
                                            if let Some(dur) = song.duration {
                                                ui.label(format!("{}:{:02}", dur / 60, dur % 60));
                                            } else {
                                                ui.label("-");
                                            }
                                        });
                                    });
                                });
                            }
                            LoadingState::Loading => {
                                table.body(|body| {
                                    body.rows(20.0, 1, |mut row| {
                                        row.col(|ui| {
                                            ui.label("Loading...");
                                        });
                                    });
                                });
                            }
                            LoadingState::Failed(_) => {
                                table.body(|body| {
                                    body.rows(20.0, 1, |mut row| {
                                        row.col(|ui| {
                                            ui.label("Failed to load.");
                                        });
                                    });
                                });
                            }
                        }
                    });
                });
            });
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        songs: &LazySongDatabase,
        theme: &ThemeManager,
        on_restore: impl FnOnce(AppState),
        on_play_suggested: impl Fn(Vec<SongDTO>, Uuid),
    ) {
        ui.vertical(|ui| {

            ui.heading("Welcome back!");
            ui.add_space(10.0);

            // Previous session UI
            if let Some(state) = load_app_state() {
                Frame::new()
                    .fill(theme.background_elevated)
                    .corner_radius(8.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_width(ui.available_width());
                            ui.vertical(|ui| {
                                ui.set_width(170.0);
                                if let Some(uuid) = state.current_song_uuid {
                                    if let LoadingState::Loaded(song) =
                                        songs.get(&uuid, |s| s.cover_art.clone())
                                    {
                                        if let Some(art) = song {
                                            self.resolve_and_render_art(ui, &art, Vec2::new(150.0, 150.0));
                                        } else {
                                            ui.allocate_exact_size(Vec2::new(150.0, 150.0), Sense::hover());
                                        }
                                    } else {
                                        ui.allocate_exact_size(Vec2::new(150.0, 150.0), Sense::hover());
                                    }
                                } else {
                                    ui.allocate_exact_size(Vec2::new(150.0, 150.0), Sense::hover());
                                }
                                ui.label(RichText::new("Resume Session").size(18.0).strong());
                            });

                            ui.add_space(20.0);
                            ui.vertical(|ui| {
                                ui.heading("Continue where you left off");
                                if let Some(name) = &state.playlist_name {
                                    ui.label(RichText::new(format!("Playlist: {}", name)).size(18.0).strong());
                                }

                                if let Some(uuid) = state.current_song_uuid {
                                    let song_info = songs.get(&uuid, |s| {
                                        (
                                            s.title.to_string(),
                                            s.original_artists.clone(),
                                        )
                                    });
                                    match song_info {
                                        LoadingState::Loaded((title, artists)) => {
                                            ui.heading(title);
                                            ui.label(artists.join(" & "));
                                        }
                                        _ => {
                                            ui.label("Loading last song details...");
                                        }
                                    }
                                }

                                ui.add_space(10.0);

                                ui.horizontal(|ui| {
                                    let play_resp = ui.add(
                                        Button::image(
                                            Image::new(include_image!("../../assets/play.svg"))
                                                .fit_to_exact_size(Vec2::new(20.0, 20.0)),
                                        )
                                            .min_size(Vec2::new(40.0, 40.0))
                                            .corner_radius(20.0)
                                            .fill(theme.primary),
                                    );
                                    if play_resp.clicked() { on_restore(state); }
                                });
                            });
                        });
                    });
                ui.add_space(20.0);
            }

            // Suggested Songs UI
            self.render_song_list(
                ui,
                theme,
                "suggested",
                "Suggested songs",
                "Personalized songs curated based on your listening taste",
                &self.suggested_songs,
                &on_play_suggested,
            );

            // Trending Songs UI
            ui.add_space(20.0);
            self.render_song_list(
                ui,
                theme,
                "trending",
                "Trending songs",
                "Top songs in the last week",
                &self.trending_songs,
                &on_play_suggested,
            );

            // Setlist Songs UI
            ui.add_space(20.0);
            let setlist_label = self
                .setlist_name
                .blocking_lock()
                .clone()
                .unwrap_or_else(|| "Featured setlist for the current year".to_string());
            self.render_song_list(
                ui,
                theme,
                "setlist",
                "Most Recent Setlist",
                &setlist_label,
                &self.setlist_songs,
                &on_play_suggested,
            );
        });
    }
}

