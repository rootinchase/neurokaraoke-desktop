use crate::activity::resolve_and_render_art;
use crate::api::{
    Artwork, AzuraCastNowPlayingResponse, AzuraCastTrackInfo, GameHubScheduledInfo,
    LazySongDatabase, LoadingState, RadioCurrentStateResponse, SongDTO,
};
use crate::audio::Player;
use crate::config::{Config, RadioUrl};
use crate::theme::ThemeManager;
use crate::utilities::cache::{AssetType, PersistentMediaCache};
use crate::utilities::persistence;
use eframe::egui::{
    Button, Color32, Context, Frame, Image, RichText, ScrollArea, Ui, Vec2, include_image,
};
use reqwest::Client;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct RadioActivity {
    pub ctx: Context,
    pub cache: Arc<PersistentMediaCache>,
    pub rt: Arc<Runtime>,
    pub client: Client,
    pub songs: LazySongDatabase,
    pub player: Player,
    pub current_state: Arc<Mutex<LoadingState<RadioCurrentStateResponse>>>,
    pub azuracast_state: Arc<Mutex<LoadingState<AzuraCastNowPlayingResponse>>>,
    pub gamehub_schedule: Arc<Mutex<LoadingState<GameHubScheduledInfo>>>,
}

impl RadioActivity {
    pub fn new(
        ctx: Context,
        cache: Arc<PersistentMediaCache>,
        rt: Arc<Runtime>,
        client: Client,
        songs: LazySongDatabase,
        player: Player,
    ) -> Self {
        let current_state = Arc::new(Mutex::new(
            LoadingState::<RadioCurrentStateResponse>::Loading,
        ));
        let azuracast_state = Arc::new(Mutex::new(
            LoadingState::<AzuraCastNowPlayingResponse>::Loading,
        ));
        let gamehub_schedule = Arc::new(Mutex::new(LoadingState::<GameHubScheduledInfo>::Loading));
        let last_radio_art_uuid = Arc::new(Mutex::new(None));

        // Spawn background polling loop for real-time track changes, metadata, and timing updates
        let songs_clone = songs.clone();
        let current_state_clone = current_state.clone();
        let azuracast_state_clone = azuracast_state.clone();
        let gamehub_schedule_clone = gamehub_schedule.clone();
        let last_radio_art_uuid_clone = last_radio_art_uuid.clone();
        let cache_clone_for_cleanup = cache.clone();
        let ctx_clone = ctx.clone();

        rt.spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;

                // 1. Fetch AzuraCast now playing (primary for stream timing and track changes)
                if let Ok(res) = songs_clone.get_azuracast_now_playing().await {
                    let mut guard = azuracast_state_clone.lock().await;
                    let should_repaint = match &*guard {
                        LoadingState::Loaded(prev) => {
                            let prev_sh = prev
                                .effective_now_playing()
                                .as_ref()
                                .and_then(|np| np.sh_id);
                            let new_sh =
                                res.effective_now_playing().as_ref().and_then(|np| np.sh_id);
                            prev_sh != new_sh
                        }
                        _ => true,
                    };

                    if should_repaint {
                        if let Some(np) = res.effective_now_playing() {
                            let is_radio_specific = np
                                .song
                                .as_ref()
                                .and_then(|s| s.custom_fields.as_ref())
                                .and_then(|cf| cf.get("songId"))
                                .map(|sid| {
                                    let t = sid.trim();
                                    t.is_empty() || t.eq_ignore_ascii_case("null")
                                })
                                .unwrap_or(true);

                            if let Some(old_uuid) = last_radio_art_uuid_clone.lock().await.take() {
                                cache_clone_for_cleanup.remove_asset(old_uuid, AssetType::Image);
                            }

                            if is_radio_specific {
                                if let Some(song) = &np.song {
                                    if let Some(art_url) = &song.art {
                                        let new_uuid =
                                            Uuid::new_v5(&Uuid::NAMESPACE_URL, art_url.as_bytes());
                                        *last_radio_art_uuid_clone.lock().await = Some(new_uuid);
                                    }
                                }
                            }
                        }
                    }

                    *guard = LoadingState::Loaded(res);
                    if should_repaint {
                        ctx_clone.request_repaint();
                    }
                }

                // 2. Fetch Radio Current State (primary for rich song metadata and cover art)
                if let Ok(res) = songs_clone.get_radio_current_state().await {
                    let mut guard = current_state_clone.lock().await;
                    let should_repaint = match &*guard {
                        LoadingState::Loaded(prev) => {
                            let prev_id = prev.current.as_ref().map(|s| s.id);
                            let new_id = res.current.as_ref().map(|s| s.id);
                            prev_id != new_id
                        }
                        _ => true,
                    };
                    *guard = LoadingState::Loaded(res);
                    if should_repaint {
                        ctx_clone.request_repaint();
                    }
                }

                // 3. Fetch Gamehub schedule
                if let Ok(res) = songs_clone.get_gamehub_schedule().await {
                    let mut guard = gamehub_schedule_clone.lock().await;
                    *guard = LoadingState::Loaded(res);
                }
            }
        });

        Self {
            ctx,
            cache,
            rt,
            client,
            songs,
            player,
            current_state,
            azuracast_state,
            gamehub_schedule,
        }
    }

    pub fn fetch_data(&self) {
        let songs = self.songs.clone();
        let current_state = self.current_state.clone();
        let azuracast_state = self.azuracast_state.clone();
        let gamehub_schedule = self.gamehub_schedule.clone();
        let ctx = self.ctx.clone();

        self.rt.spawn(async move {
            match songs.get_radio_current_state().await {
                Ok(res) => {
                    *current_state.lock().await = LoadingState::Loaded(res);
                }
                Err(e) => {
                    *current_state.lock().await = LoadingState::Failed(Arc::new(e));
                }
            }

            match songs.get_azuracast_now_playing().await {
                Ok(res) => {
                    *azuracast_state.lock().await = LoadingState::Loaded(res);
                }
                Err(e) => {
                    *azuracast_state.lock().await = LoadingState::Failed(Arc::new(e));
                }
            }

            match songs.get_gamehub_schedule().await {
                Ok(res) => {
                    *gamehub_schedule.lock().await = LoadingState::Loaded(res);
                }
                Err(e) => {
                    *gamehub_schedule.lock().await = LoadingState::Failed(Arc::new(e));
                }
            }

            ctx.request_repaint();
        });
    }

    pub fn render(&self, ui: &mut Ui, theme: &ThemeManager, config: &mut Config) {
        let current_state = self.current_state.clone();
        let azuracast_state = self.azuracast_state.clone();
        let cache = self.cache.clone();
        let rt = self.rt.clone();
        let client = self.client.clone();
        let ctx = self.ctx.clone();

        ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Neuro 21 Station").color(theme.text).strong());
                ui.add_space(20.0);

                ui.label(RichText::new("Stream:").color(theme.text_secondary));
                eframe::egui::ComboBox::from_id_salt("radio_url_selector")
                    .selected_text(config.radio_url.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut config.radio_url,
                            RadioUrl::Mp3320Kbps,
                            RadioUrl::Mp3320Kbps.name(),
                        );
                        ui.selectable_value(
                            &mut config.radio_url,
                            RadioUrl::Opus,
                            RadioUrl::Opus.name(),
                        );
                    });
            });
            ui.add_space(16.0);

            // Fetch data if not loaded
            let current_guard = current_state.try_lock();
            let azuracast_guard = azuracast_state.try_lock();

            if let Ok(curr) = &current_guard {
                if matches!(**curr, LoadingState::Loading) {
                    drop(current_guard);
                    drop(azuracast_guard);
                    self.fetch_data();
                    ui.label("Loading radio state...");
                    return;
                }
            }

            ui.horizontal(|ui| {
                if let Ok(LoadingState::Loaded(az)) = azuracast_guard.as_deref() {
                    let listeners = az.listeners.as_ref().map(|l| l.current).unwrap_or(0);
                    let is_online = az.is_online;
                    ui.label(
                        RichText::new(if is_online {
                            "Station Online"
                        } else {
                            "Station Offline"
                        })
                        .size(14.0),
                    );
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new(format!("👥 Listeners: {}", listeners))
                            .size(14.0)
                            .color(theme.text_secondary),
                    );
                }
            });

            ui.add_space(16.0);

            let az_np = if let Ok(LoadingState::Loaded(az)) = azuracast_guard.as_deref() {
                az.effective_now_playing()
            } else {
                None
            };

            let socket_curr = if let Ok(LoadingState::Loaded(curr)) = current_guard.as_deref() {
                curr.current.as_ref()
            } else {
                None
            };

            render_radio_now_playing(
                ui,
                theme,
                &cache,
                &ctx,
                &rt,
                &client,
                &self.player,
                config,
                az_np.as_ref(),
                socket_curr,
            );

            if let Ok(LoadingState::Loaded(curr)) = current_guard.as_deref() {
                if !curr.upcoming.is_empty() {
                    ui.add_space(16.0);
                    ui.heading(RichText::new("Upcoming Queue").size(16.0).color(theme.text));
                    ui.add_space(8.0);
                    for song in &curr.upcoming {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(song.title.as_ref()).color(theme.text));
                        });
                    }
                }

                if !curr.history.is_empty() {
                    ui.add_space(16.0);
                    ui.heading(
                        RichText::new("Recently Played")
                            .size(16.0)
                            .color(theme.text),
                    );
                    ui.add_space(8.0);
                    for song in &curr.history {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(song.title.as_ref()).color(theme.text_secondary),
                            );
                        });
                    }
                }
            } else if let Ok(LoadingState::Failed(e)) = current_guard.as_deref() {
                ui.label(
                    RichText::new(format!("Failed to load radio state: {}", e)).color(Color32::RED),
                );
            }
        });
    }
}

pub fn render_radio_now_playing(
    ui: &mut Ui,
    theme: &ThemeManager,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    player: &Player,
    config: &Config,
    az_np: Option<&AzuraCastTrackInfo>,
    socket_curr: Option<&SongDTO>,
) {
    if az_np.is_some() || socket_curr.is_some() {
        Frame::new()
            .fill(theme.background_elevated)
            .corner_radius(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 20.0);
                ui.set_max_height(120.0);
                ui.horizontal_centered(|ui| {
                    //Insert play button here
                    let resp = ui.add(
                        Button::image(
                            Image::new(include_image!("../../assets/play.svg"))
                                .fit_to_exact_size(Vec2::new(50.0, 50.0)),
                        )
                        .min_size(Vec2::new(70.0, 70.0))
                        .corner_radius(40.0)
                        .fill(theme.primary),
                    );

                    if resp.clicked() {
                        let _ = persistence::clear_app_state();
                        player.radio_stream(config.radio_url.url().to_string(), Player::play);
                    }
                    ui.add_space(16.0);

                    // Render artwork from socket song if available
                    if let Some(song) = socket_curr {
                        if let Some(art) = &song.cover_art {
                            resolve_and_render_art(
                                ui,
                                cache,
                                ctx,
                                rt,
                                client,
                                art,
                                Vec2::new(120.0, 120.0),
                            );
                        }
                    } else if let Some(np) = az_np.as_ref() {
                        if let Some(song) = &np.song {
                            if let Some(art_url) = &song.art {
                                let song_id_uuid = song
                                    .custom_fields
                                    .as_ref()
                                    .and_then(|cf| cf.get("songId"))
                                    .and_then(|sid| {
                                        let trimmed = sid.trim();
                                        if trimmed.is_empty()
                                            || trimmed.eq_ignore_ascii_case("null")
                                        {
                                            None
                                        } else {
                                            Uuid::parse_str(trimmed).ok()
                                        }
                                    });

                                let art_uuid = song_id_uuid.unwrap_or_else(|| {
                                    Uuid::new_v5(&Uuid::NAMESPACE_URL, art_url.as_bytes())
                                });

                                let art_obj = Artwork {
                                    id: art_uuid.to_string(),
                                    file_name: art_url.clone().into(),
                                    cloudflare_id: Some(art_uuid.to_string().into()),
                                    absolute_path: art_url.clone().into(),
                                    artist: None,
                                    is_sensitive: false,
                                };
                                resolve_and_render_art(
                                    ui,
                                    cache,
                                    ctx,
                                    rt,
                                    client,
                                    &art_obj,
                                    Vec2::new(120.0, 120.0),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    Vec2::new(120.0, 120.0),
                                    eframe::egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, 4.0, theme.background_elevated);
                                ui.painter().text(
                                    rect.center(),
                                    eframe::egui::Align2::CENTER_CENTER,
                                    "🎵",
                                    eframe::egui::FontId::proportional(32.0),
                                    Color32::WHITE,
                                );
                            }
                        }
                    }

                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Neuro 21 Station")
                                .size(18.0)
                                .color(theme.primary)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("NOW PLAYING")
                                .size(12.0)
                                .color(theme.primary)
                                .strong(),
                        );
                        ui.add_space(4.0);

                        let title = az_np
                            .as_ref()
                            .and_then(|np| np.song.as_ref())
                            .and_then(|s| s.title.clone())
                            .or_else(|| socket_curr.map(|s| s.title.to_string()))
                            .unwrap_or_else(|| "Unknown Track".to_string());

                        ui.label(RichText::new(title).size(18.0).color(theme.text).strong());

                        let artist = az_np
                            .as_ref()
                            .and_then(|np| np.song.as_ref())
                            .and_then(|s| s.artist.clone())
                            .or_else(|| socket_curr.map(|s| s.original_artists.join(", ")));

                        if let Some(art_str) = artist {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(format!("Artist: {}", art_str))
                                    .size(14.0)
                                    .color(theme.text_secondary),
                            );
                        }

                        if let Some(np) = az_np.as_ref() {
                            let elapsed = np.elapsed_secs();
                            let duration = np.duration_secs();
                            let remaining = np.remaining_secs();
                            if duration > 0 {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format!(
                                        "Progress: {}:{:02} / {}:{:02} (-{}:{:02})",
                                        elapsed / 60,
                                        elapsed % 60,
                                        duration / 60,
                                        duration % 60,
                                        remaining / 60,
                                        remaining % 60
                                    ))
                                    .size(12.0)
                                    .color(theme.text_muted),
                                );
                            }
                        }
                    });
                });
            });
    } else {
        ui.label("Radio is currently offline or between tracks.");
    }
}
