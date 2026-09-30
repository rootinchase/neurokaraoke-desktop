use crate::config::Config;
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::PersistentMediaCache;
use eframe::egui::{self, Color32, Frame, ProgressBar, RichText, ScrollArea, Ui};
use std::sync::Arc;
use tokio::runtime::Runtime;

pub struct SettingsActivity {
    cache: Arc<PersistentMediaCache>,
    storage_info: Option<(u64, u64, u64, u64)>, // (total, free, our_usage, other_usage)
    clear_cache_status: Option<String>,
}

impl SettingsActivity {
    pub fn new(cache: Arc<PersistentMediaCache>) -> Self {
        let mut activity = Self {
            cache,
            storage_info: None,
            clear_cache_status: None,
        };
        activity.refresh_storage();
        activity
    }

    pub fn refresh_storage(&mut self) {
        let base_dir = crate::utilities::cache::cache_dir();

        let total = fs2::total_space(base_dir).unwrap_or(0);
        let free = fs2::free_space(base_dir).unwrap_or(0);
        let our_usage = get_dir_size(base_dir);
        let other_usage = total.saturating_sub(free).saturating_sub(our_usage);

        self.storage_info = Some((total, free, our_usage, other_usage));
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        theme: &ThemeManager,
        config: &mut Config,
        rt: &Arc<Runtime>,
    ) {
        ScrollArea::vertical().show(ui, |ui| {
            // Card 1: Application Settings
            Frame::new()
                .fill(theme.background_elevated)
                .corner_radius(8.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading("Application Settings");
                    ui.add_space(10.0);

                    let mut changed = false;

                    // Unfocused framerate
                    ui.horizontal(|ui| {
                        ui.label("Framerate when not focused (FPS):");
                        let mut fps = config.framerate_when_not_focused;
                        if ui
                            .add(egui::Slider::new(&mut fps, 0.0..=60.0).step_by(1.0))
                            .changed()
                        {
                            config.framerate_when_not_focused = fps;
                            changed = true;
                        }
                    });
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("0.0 means unlimited/unthrottled when unfocused.")
                            .size(12.0)
                            .color(theme.text_muted),
                    );

                    if changed {
                        let _ = config.write();
                    }
                });

            ui.add_space(15.0);

            // Card 2: Cache Settings
            Frame::new()
                .fill(theme.background_elevated)
                .corner_radius(8.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading("Cache Settings");
                    ui.add_space(10.0);

                    let cache_config = config.cache.clone();
                    let mut cc = if tokio::runtime::Handle::try_current().is_ok() {
                        tokio::task::block_in_place(|| cache_config.blocking_lock())
                    } else {
                        cache_config.blocking_lock()
                    };

                    let mut cache_changed = false;

                    ui.horizontal(|ui| {
                        ui.label("Cache Size Limit (MB):");
                        let mut limit = cc.cache_size_limit_mb;
                        if ui
                            .add(egui::DragValue::new(&mut limit).speed(10.0).range(50..=50000))
                            .changed()
                        {
                            cc.cache_size_limit_mb = limit;
                            cache_changed = true;
                        }
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Cache Expiration (Seconds):");
                        let mut exp = cc.cache_expiration_secs;
                        if ui
                            .add(
                                egui::DragValue::new(&mut exp)
                                    .speed(3600.0)
                                    .range(300..=2592000),
                            )
                            .changed()
                        {
                            cc.cache_expiration_secs = exp;
                            cache_changed = true;
                        }
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Cache Sweep Interval (Seconds):");
                        let mut sweep = cc.cache_sweep_interval_secs;
                        if ui
                            .add(
                                egui::DragValue::new(&mut sweep)
                                    .speed(10.0)
                                    .range(10..=3600),
                            )
                            .changed()
                        {
                            cc.cache_sweep_interval_secs = sweep;
                            cache_changed = true;
                        }
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Song DB Update Interval (Seconds):");
                        let mut db_int = cc.song_database_update_interval_secs;
                        if ui
                            .add(
                                egui::DragValue::new(&mut db_int)
                                    .speed(60.0)
                                    .range(60..=86400),
                            )
                            .changed()
                        {
                            cc.song_database_update_interval_secs = db_int;
                            cache_changed = true;
                        }
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label("Playlist Cache TTL (Seconds):");
                        let mut pl_ttl = cc.playlist_cache_ttl_secs;
                        if ui
                            .add(
                                egui::DragValue::new(&mut pl_ttl)
                                    .speed(3600.0)
                                    .range(300..=604800),
                            )
                            .changed()
                        {
                            cc.playlist_cache_ttl_secs = pl_ttl;
                            cache_changed = true;
                        }
                    });

                    drop(cc);

                    if cache_changed {
                        let _ = config.write();
                    }
                });

            ui.add_space(15.0);

            // Card 3: Storage Usage & Cache Management
            Frame::new()
                .fill(theme.background_elevated)
                .corner_radius(8.0)
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.heading("Storage Usage & Cache Management");
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("🔄 Refresh Storage Stats").clicked() {
                            self.refresh_storage();
                        }
                    });

                    ui.add_space(10.0);

                    if let Some((total, free, our_usage, other_usage)) = self.storage_info {
                        ui.label(format!("Total Drive Storage: {}", format_bytes(total)));
                        ui.add_space(4.0);
                        ui.label(format!("Available / Free Space: {}", format_bytes(free)));
                        ui.add_space(4.0);
                        ui.label(format!("Our App Cache Usage: {}", format_bytes(our_usage)));
                        ui.add_space(4.0);
                        ui.label(format!("Other Programs Usage: {}", format_bytes(other_usage)));

                        ui.add_space(12.0);

                        // Visual progress bar representation (Our usage vs Other vs Free)
                        if total > 0 {
                            let our_ratio = (our_usage as f32 / total as f32).clamp(0.0, 1.0);
                            let other_ratio = (other_usage as f32 / total as f32).clamp(0.0, 1.0);
                            let used_ratio = (our_ratio + other_ratio).clamp(0.0, 1.0);

                            ui.label("Storage Breakdown:");
                            ui.add_space(4.0);
                            ui.add(
                                ProgressBar::new(used_ratio).text(format!(
                                    "Used: {:.1}% (Ours: {}, Other: {})",
                                    used_ratio * 100.0,
                                    format_bytes(our_usage),
                                    format_bytes(other_usage)
                                )),
                            );
                        }
                    } else {
                        ui.label("Storage info unavailable.");
                    }

                    ui.add_space(15.0);

                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("🗑 Clear Cache").color(Color32::RED))
                            .clicked()
                        {
                            let cache_clone = self.cache.clone();
                            let rt_clone = rt.clone();
                            let ctx = ui.ctx().clone();
                            rt_clone.spawn(async move {
                                match cache_clone.clear_cache().await {
                                    Ok(_) => {
                                        debug_log!("🧹 Cache cleared successfully!");
                                    }
                                    Err(e) => {
                                        debug_log!("❌ Failed to clear cache: {}", e);
                                    }
                                }
                                ctx.request_repaint();
                            });
                            self.clear_cache_status = Some("Cache cleared!".to_string());
                            self.refresh_storage();
                        }

                        if let Some(msg) = &self.clear_cache_status {
                            ui.add_space(10.0);
                            ui.label(RichText::new(msg).color(Color32::GREEN));
                        }
                    });
                });

            ui.add_space(20.0);
        });
    }
}

fn get_dir_size(path: &std::path::Path) -> u64 {
    let mut total_size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    total_size += get_dir_size(&entry.path());
                } else {
                    total_size += metadata.len();
                }
            }
        }
    }
    total_size
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
