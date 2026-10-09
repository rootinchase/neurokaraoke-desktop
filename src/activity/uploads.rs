use crate::activity::SortOption;
use crate::api::{LazySongDatabase, LoadingState, SongDTO, UserLimits};
use crate::audio::types::Player;
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::cache_dir;
use crate::utilities::util::{load_cached_song_list, sort_items};

use eframe::egui::{Context, ProgressBar, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};
use ron::ser::to_string_pretty;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::fs::write;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct UploadsActivity {
    pub ctx: Context,
    pub db: LazySongDatabase,
    pub songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub limits: Arc<Mutex<Option<UserLimits>>>,
    pub is_fetching: Arc<AtomicBool>,
    pub current_sort: SortOption,
    pub current_sort_desc: bool,
}

impl UploadsActivity {
    pub fn new(ctx: Context, db: LazySongDatabase) -> Self {
        Self {
            ctx,
            db,
            songs: Arc::new(Mutex::new(LoadingState::Loading)),
            limits: Arc::new(Mutex::new(None)),
            is_fetching: Arc::new(AtomicBool::new(false)),
            current_sort: SortOption::Date,
            current_sort_desc: true,
        }
    }

    /// Loads the uploads list (cache-first) and the user's quota, then repaints.
    pub fn fetch_uploads(&self) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }

        let s = self.songs.clone();
        let l = self.limits.clone();
        let db = self.db.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            let cache_path = cache_dir().join("uploads.ron");
            let has_cached = load_cached_song_list(&cache_path, &s).await;

            match db.get_user_uploads().await {
                Ok(list) => {
                    *s.lock().await = LoadingState::Loaded(list.clone());
                    if let Some(parent) = cache_path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    if let Ok(serialized) = to_string_pretty(&list, Default::default()) {
                        let _ = write(&cache_path, serialized).await;
                    }
                }
                Err(err) => {
                    let mut lock = s.lock().await;
                    if !has_cached && matches!(*lock, LoadingState::Loading) {
                        *lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }

            // Best-effort quota fetch; ignore failures so the list still renders.
            if let Ok(limits) = db.get_user_limits().await {
                *l.lock().await = Some(limits);
            }

            ctx.request_repaint();
        });
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        theme: &ThemeManager,
        player: &mut Player,
        current_song_uuid: &Option<Uuid>,
        is_logged_in: bool,
    ) {
        ui.heading("Uploads");
        ui.add_space(10.0);

        if !is_logged_in {
            ui.label("Sign in to view your uploads.");
            return;
        }

        // Quota header from UserLimits — Songs and Storage bars side by side
        if let Some(limits) = self.limits.blocking_lock().as_ref() {
            let song_fraction = if limits.max_songs > 0 {
                limits.current_song_count as f32 / limits.max_songs as f32
            } else {
                0.0
            };
            let storage_fraction = if limits.max_storage_bytes > 0 {
                limits.used_storage_bytes as f32 / limits.max_storage_bytes as f32
            } else {
                0.0
            };

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label("Songs");
                    let bar = ProgressBar::new(song_fraction)
                        .text(format!(
                            "{} / {}",
                            limits.current_song_count, limits.max_songs
                        ))
                        .fill(theme.primary);
                    ui.add_sized([200.0, 20.0], bar);
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label("Storage");
                    let bar = ProgressBar::new(storage_fraction)
                        .text(format!(
                            "{} / {}",
                            format_bytes(limits.used_storage_bytes),
                            format_bytes(limits.max_storage_bytes)
                        ))
                        .fill(theme.primary);
                    ui.add_sized([200.0, 20.0], bar);
                });
            });
            ui.add_space(10.0);
        }

        let guard = self.songs.blocking_lock();
        match &*guard {
            LoadingState::Loaded(loaded) => {
                let mut songs = loaded.clone();
                sort_items(
                    &mut songs,
                    &self.current_sort,
                    &self.current_sort_desc,
                    |s| s.title.to_string(),
                    |_| 0,
                    |s| s.play_count.unwrap_or(0) as u32,
                    |s| s.stream_date.as_ref().map(|d| d.to_string()),
                );

                if songs.is_empty() {
                    ui.label("You haven't uploaded any songs yet.");
                    return;
                }

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        TableBuilder::new(ui)
                            .column(Column::exact(24.0))
                            .column(Column::auto())
                            .column(Column::auto())
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.label("");
                                });
                                header.col(|ui| {
                                    ui.label("Title");
                                });
                                header.col(|ui| {
                                    ui.label("Original");
                                });
                            })
                            .body(|body| {
                                body.rows(20.0, songs.len(), |mut row| {
                                    let song = &songs[row.index()];
                                    let song_uuid = song.id;

                                    row.col(|ui| {
                                        let mut is_in_playlist = player
                                            .get_playlist()
                                            .is_some_and(|pl| pl.contains(&song_uuid));
                                        if ui.checkbox(&mut is_in_playlist, "").changed() {
                                            if is_in_playlist {
                                                player.append_to_playlist(song_uuid);
                                                if player.get_playback_state().is_none() {
                                                    player.play_song(song_uuid);
                                                } else {
                                                    player.play();
                                                }
                                            } else {
                                                player.remove_from_playlist(song_uuid);
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        if ui
                                            .selectable_label(
                                                current_song_uuid == &Some(song_uuid),
                                                song.title.to_string(),
                                            )
                                            .clicked()
                                        {
                                            player.play_song(song_uuid);
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(song.original_artists.join(" & "));
                                    });
                                });
                            });
                    });
            }
            LoadingState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading uploads...");
                });
            }
            LoadingState::Failed(err) => {
                debug_log!("Failed to load uploads: {}", err);
                ui.label(format!("Error loading uploads: {}", err));
            }
        }
    }
}

/// Human-readable byte count (KB / MB / GB).
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{} B", bytes)
    }
}
