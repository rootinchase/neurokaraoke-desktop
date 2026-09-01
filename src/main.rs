mod theme;
#[allow(dead_code)]
mod activity; // not finished yet, hence we allow dead code so we don't get warned a bunch
mod audio;
mod api;
mod util;
mod cache;
mod config;

use std::collections::HashMap;
use crate::activity::{ActivityType, home::HomeActivity, playlist::PlaylistActivity};
use crate::api::{LazySongDatabase, LoadingState, Song};
use crate::audio::{Player, PlaybackState, LoopMode};
use crate::cache::Cache;
use crate::config::Config;
use crate::theme::{SelectableTheme, ThemeManager};
use eframe::egui::{include_image, lerp, Align, Color32, CornerRadius, CursorIcon, ImageSource, Layout, PopupKind, Pos2, RectAlign, Rgba, RichText, Sense, Stroke, TextWrapMode, Ui, Vec2};
use eframe::{egui, Frame};
use mimalloc::MiMalloc;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;
use uuid::Uuid;

// For media controls on Linux, macOS, and Windows
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig, MediaPosition};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> eframe::Result<()> {
    cache::init_cache_dir();
    config::init_config_dir();

    let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap());

    let _guard = runtime.enter();

    App::run(runtime)
}

pub struct App {
    cache: Arc<Cache>,
    songs: LazySongDatabase,
    player: Player,
    media_controls: Option<MediaControls>,

    search: String,
    dragging_seeker: bool,
    dragging_volume: bool,

    // theme stuff
    theme: ThemeManager,

    // activity stuff
    activity: ActivityType,
    home_activity: HomeActivity,
    playlist_activity: PlaylistActivity,

    current_song_uuid: Option<Uuid>,
    current_playback_state: Option<PlaybackState>,
    last_os_playback_update: Instant,

    rt: Arc<tokio::runtime::Runtime>,
    client: Client,
    pub config: Config,
}


impl App {
    fn new(creation_context: &eframe::CreationContext<'_>, ctx: &egui::Context, rt: Arc<tokio::runtime::Runtime>) -> Self {
        egui_extras::install_image_loaders(ctx);
        // ... (skipping font loading, same as before) ...
        let mut fonts = egui::FontDefinitions::default();

        fonts.font_data.insert(
            "noto-sans-jp".to_string(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/NotoSansJP-Regular.ttf")).into()
        );

        fonts.font_data.insert(
            "Roboto".to_string(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Roboto-VariableFont_wdth,wght.ttf")).into()
        );

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "noto-sans-jp".to_owned());

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Roboto".to_owned());

        ctx.set_fonts(fonts);

        #[cfg(debug_assertions)]
        ctx.global_style_mut(|s| s.debug.warn_if_rect_changes_id = false); // workaround for https://github.com/emilk/egui/issues/8092

        let config = Config::read().unwrap_or_default();

        let cache = Cache::load_or_default();
        let client = Client::new();
        let songs = LazySongDatabase::new(client.clone(), Arc::new(Cache::load_or_default_custom::<HashMap<Uuid, Song>>("songs.ron").into_iter().map(|(uuid, song)| (uuid, LoadingState::Loaded(song))).collect()));

        let player = Player::new(rt.clone(), ctx.clone(), songs.clone(), cache.clone());
        player.volume(config.volume);
        player.shuffle(config.shuffle);
        player.looping(config.loop_mode);

        let s = songs.clone();
        let c = cache.clone();
        let cc = config.cache.clone();
        let last_update: Arc<Mutex<Option<Instant>>> = Arc::default();
        cache.clone().create_worker(move || {
            let s = s.clone();
            let c = c.clone();
            let cc = cc.clone();
            let last_update = last_update.clone();
            async move {
                let d = Duration::from_secs(cc.lock().await.cache_expiration_secs);
                if last_update.lock().await.map(|i| Instant::now() - i >= d).unwrap_or(true) && c.is_online() {
                    last_update.lock().await.replace(Instant::now());
                    s.load_all(|_| ()).await.unwrap();
                    tokio::fs::write(cache::cache_dir().join("songs.ron"), ron::ser::to_string_pretty(&s, Default::default()).unwrap())
                        .await
                        .unwrap();
                }
            }
        }, rt.handle().clone(), client.clone(), config.cache.clone());

        let mut media_controls = init_souvlaki();
        if let Some(controls) = &mut media_controls {
            let player_clone = player.clone();
            if let Err(e) = controls.attach(move |event| {
                match event {
                    MediaControlEvent::Play => { player_clone.play() },
                    MediaControlEvent::Pause => { player_clone.pause(); },
                    MediaControlEvent::Toggle => {
                        if let Some(state) = player_clone.get_playback_state() {
                            if state.paused() { player_clone.play(); }
                            else { player_clone.pause(); }
                        }
                    },
                    MediaControlEvent::Next => player_clone.next_song(),
                    MediaControlEvent::Previous => player_clone.previous(),
                    MediaControlEvent::SetVolume(vol) => player_clone.volume(vol as f32),
                    _ => {}
                }
            }) {
                eprintln!("Failed to attach media controls: {:?}", e);
            }
        }

        Self {
            cache,
            songs: songs.clone(),
            player,

            media_controls,
            search: "".to_string(),
            dragging_seeker: false,
            dragging_volume: false,

            theme: ThemeManager::new(config.theme.as_theme()),

            activity: ActivityType::Home,
            home_activity: HomeActivity::new(ctx.clone()),
            playlist_activity: PlaylistActivity::new(ctx.clone(), songs),

            current_song_uuid: None,
            current_playback_state: None,
            last_os_playback_update: Instant::now(),

            rt,
            client,
            config,
        }
    }



    pub fn update_os_metadata(&mut self, title: &str, artist: &str, duration_secs: u64, cover_url: &str ) {
        if let Some(controls) = &mut self.media_controls {
            let meta = MediaMetadata {
                title: Some(title),
                artist: Some(artist),
                album: Some("NeuroKaraoke Live"),
                duration: Some(Duration::from_secs(duration_secs)),
                cover_url: Some(cover_url),
            };
            let _ = controls.set_metadata(meta);
        }
    }

    pub fn update_os_playback(&mut self) {
        if let Some(controls) = &mut self.media_controls {
             if let Some(state) = self.player.get_playback_state() {
                let playback = if state.paused() {
                    MediaPlayback::Paused { progress: Some(MediaPosition(state.position())) }
                } else {
                    MediaPlayback::Playing { progress: Some(MediaPosition(state.position())) }
                };
                let _ = controls.set_playback(playback);
             }
        }
    }
    fn run(rt: Arc<tokio::runtime::Runtime>) -> eframe::Result<()> {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("Invalid icon");

        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_icon(icon)
                .with_min_inner_size(Vec2::new(640.0, 480.0)),
            ..Default::default()
        };

        eframe::run_native(
            "Karaoke App",
            options,
            Box::new(|cc| Ok(Box::new(Self::new( cc, &cc.egui_ctx, rt)))),
        )
    }
}

fn init_souvlaki() -> Option<MediaControls> {
    let hwnd_ptr: Option<*mut std::ffi::c_void> = None;

    #[cfg(target_os = "windows")]
    {
        // Extract raw pointer from eframe's 0.6 handle to pass down to souvlaki's structure
        if let Ok(window_handle) = cc.integration_info.window_handle {
            if let Ok(RawWindowHandle::Win32(handle)) = window_handle.as_raw() {
                // Convert the NonZeroIsize HWND into a raw c_void pointer
                hwnd_ptr = Some(handle.hwnd.get() as *mut std::ffi::c_void);
            }
        }

        if hwnd_ptr.is_none() {
            eprintln!("Warning: Windows HWND could not be resolved. Media controls disabled.");
            return None;
        }
    }

    let config = PlatformConfig {
        dbus_name: "neurokaraoke.desktop",
        display_name: "NeuroKaraoke Player",
        hwnd: hwnd_ptr,
    };

    match MediaControls::new(config) {
        Ok(controls) => Some(controls),
        Err(e) => {
            eprintln!("Failed to initialize Souvlaki media sublayer: {:?}", e);
            None
        }
    }
}


impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut Frame) {
        if self.theme.animate(ui.input(|i| i.stable_dt)) {
            ui.set_visuals(self.theme.visuals());
            ui.request_repaint();
        }

        // background gradient
        let rect = ui.max_rect();

        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.left_top(), self.theme.background_mid);
        mesh.colored_vertex(rect.right_top(), self.theme.background_secondary);
        mesh.colored_vertex(rect.right_bottom(), self.theme.background_mid);
        mesh.colored_vertex(rect.left_bottom(), self.theme.background);

        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);

        ui.painter().add(egui::Shape::mesh(mesh));

        // sidebar
        egui::Panel::left("sidebar")
            .resizable(false)
            .exact_size(320.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.add(egui::Image::new(include_image!("../assets/icon.png"))
                        .fit_to_exact_size(Vec2::new(32.0, 32.0))
                        .texture_options(egui::TextureOptions::LINEAR)
                    );
                    ui.with_layout(
                        Layout::top_down(Align::Center),
                        |ui| ui.label(RichText::new(self.config.theme.karaoke_str()).color(self.theme.primary_dark).size(24.0))
                    );
                });

                ui.separator();
                ui.add_space(10.0);

                let mut nav_button = |ui: &mut Ui, activity: ActivityType| {
                    let resp = ui.scope(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(egui::Image::new(activity.icon().unwrap()).fit_to_exact_size(Vec2::new(24.0, 24.0)));
                            ui.add_space(4.0);
                            let mut text = RichText::new(activity.as_str()).size(16.0);
                            if self.activity == activity {
                                text = text.color(self.theme.primary);
                            }
                            ui.add(egui::Label::new(text).selectable(false));
                        });
                    }).response.interact(Sense::click());
                    ui.add_space(4.0);
                    if resp.hovered() {
                        ui.set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        self.activity = activity;
                    }
                };

                // nav buttons
                nav_button(ui, ActivityType::Home);
                nav_button(ui, ActivityType::Search);
                nav_button(ui, ActivityType::Playlists);

                // bottom area
                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::LEFT),
                    |ui| {
                        ui.add_space(10.0);

                        // theme switcher
                        ui.horizontal(|ui| {
                            let current = self.config.theme;
                            let mut button = |ui: &mut Ui, select_theme: SelectableTheme| {
                                let mut button = egui::Button::new(select_theme.as_str())
                                    .fill(if current == select_theme { self.theme.primary } else { self.theme.background_elevated });

                                button = match select_theme {
                                    SelectableTheme::Neuro => button.corner_radius(CornerRadius { nw: 5, sw: 5, ..Default::default() }),
                                    SelectableTheme::Evil => button.corner_radius(CornerRadius { ne: 5, se: 5, ..Default::default() }),
                                    _ => button.corner_radius(0),
                                };

                                if ui.add_sized([ui.available_width(), 0.0], button).clicked() {
                                    self.config.theme = select_theme;
                                }
                            };

                            let spacing = ui.spacing_mut();
                            let old_spacing = (spacing.item_spacing, spacing.button_padding);
                            spacing.item_spacing = Vec2::default();
                            spacing.button_padding = Vec2::new(4.0, 4.0);

                            ui.columns(3, |columns| {
                                button(&mut columns[0], SelectableTheme::Neuro);
                                button(&mut columns[1], SelectableTheme::Twins);
                                button(&mut columns[2], SelectableTheme::Evil);
                            });

                            let spacing = ui.spacing_mut();
                            (spacing.item_spacing, spacing.button_padding) = old_spacing;

                            if current != self.config.theme {
                                self.theme.set(self.config.theme.as_theme());
                            }
                        });

                        ui.add_space(16.0);

                        // profile
                        let resp = ui.scope(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::Image::new(include_image!("../assets/icon.png")) // TODO: use real pfp instead of icon
                                    .fit_to_exact_size(Vec2::new(32.0, 32.0))
                                    .corner_radius(16.0)
                                    .texture_options(egui::TextureOptions::LINEAR)
                                );

                                ui.add(egui::Label::new(RichText::new("TestUsername").size(16.0)).selectable(false));
                            });
                        }).response.interact(Sense::click());

                        if resp.hovered() {
                            ui.set_cursor_icon(CursorIcon::PointingHand);
                        }

                        if resp.clicked() {
                            self.activity = ActivityType::Profile;
                        }

                        ui.separator();
                    },
                );
            });

        if let Some(state) = self.player.get_playback_state() {
            let mut song = match self.songs.get(&state.song(), |song| song.clone()) {
                LoadingState::Loaded(s) => Some(s),
                _ => None,
            };
            
            // Fallback to URL-based metadata if database lookup failed
            if song.is_none() {
                if let Ok(meta) = self.player.current_url_metadata.lock() {
                    if let Some(meta) = &*meta {
                        // Map SongDTO to a mock Song for UI consistency, or just handle it separately in the UI
                        // For now let's just make a dummy Song
                        song = Some(crate::api::Song {
                            id: state.song(),
                            title: meta.title.clone(),
                            absolute_path: None,
                            opus: None,
                            cover_artists: Arc::from([]),
                            original_artists: Arc::from([]),
                            cover_art: None, // Need to handle cover art URL here!
                        });
                    }
                }
            }
            
            if let Some(s) = &song {
                if self.current_song_uuid != Some(state.song()) {
                    self.current_song_uuid = Some(state.song());
                    
                    let cover_art_url = if let Some(meta) = self.player.current_url_metadata.lock().unwrap().as_ref() {
                        meta.cover_art.as_ref().map(|url| url.to_string()).unwrap_or_else(|| "".to_string())
                    } else {
                        s.cover_art.as_ref()
                            .map(|ca| format!("https://images.neurokaraoke.com/WxURxyML82UkE7gY-PiBKw/{}/w=70,h=70,fit=cover,quality=90", ca.cloudflare_id))
                            .unwrap_or_else(|| "".to_string())
                    };
                    
                    self.update_os_metadata(
                        &s.title,
                        &format!("{} (feat. {})", s.original_artists.join(" & "), s.cover_artists.join(" & ")),
                        state.duration().as_secs(),
                        &cover_art_url
                    );
                }
            }

            let now = Instant::now();
            if self.current_playback_state.as_ref().map(|s| s.paused()) != Some(state.paused())
               || (now - self.last_os_playback_update > Duration::from_secs(1) && !state.paused())
            {
                 self.update_os_playback();
                 self.last_os_playback_update = now;
            }
            self.current_playback_state = Some(state);

            egui::Panel::bottom("player")
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .inner_margin(0.0)
                        .outer_margin(0.0)
                        .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                        .fill(self.theme.background_secondary)
                )
                .exact_size(80.0)
                .show(ui, |ui| {
                    // progress bar (but really fancy and overcomplicated)
                    let mut rect = ui.max_rect();
                    rect.set_height(3.0);
                    let dragging_progress = ui.pointer_latest_pos().map(|pos| (pos.x - rect.left()).clamp(0.0, rect.width()) / rect.width());
                    let position = if self.dragging_seeker && let Some(p) = dragging_progress { state.duration().mul_f32(p) } else { state.position() };
                    let progress = (position.as_millis() as f64 / state.duration().as_millis() as f64) as f32;
                    ui.painter().rect_filled(rect, 0.0, self.theme.background_elevated);
                    let mut mesh = egui::Mesh::default();
                    let lerped_color: Color32 = lerp(Rgba::from(self.theme.accent)..=Rgba::from(self.theme.primary), progress).into();
                    let w = rect.width() * progress;
                    mesh.colored_vertex(rect.left_top() + Vec2::new(0.0, 1.0), self.theme.accent);
                    mesh.colored_vertex(rect.left_top() + Vec2::new(w, 1.0), lerped_color);
                    mesh.colored_vertex(rect.left_top() + Vec2::new(w, 3.0), lerped_color);
                    mesh.colored_vertex(rect.left_top() + Vec2::new(0.0, 3.0), self.theme.accent);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    ui.painter().add(egui::Shape::mesh(mesh));

                    let resp = ui.interact(rect, ui.id().with("seeker"), Sense::click_and_drag());

                    if resp.hovered() || resp.dragged() {
                        ui.set_cursor_icon(CursorIcon::PointingHand);
                        if let Some(pos) = ui.pointer_latest_pos() && let Some(p) = dragging_progress {
                            egui::Popup::new(
                                ui.id().with("seeker_tooltip"),
                                ui.ctx().clone(),
                                egui::PopupAnchor::Position(Pos2::new(pos.x.clamp(rect.left(), rect.right()), rect.top() - 5.0)),
                                ui.layer_id(),
                            )
                                .align(RectAlign::TOP)
                                .kind(PopupKind::Tooltip)
                                .open(true)
                                .show(|ui| {
                                    let point = state.duration().mul_f32(p).as_secs();
                                    ui.add(egui::Label::new(RichText::new(format!("{}:{:02}", point / 60, point % 60)).size(12.0)).wrap_mode(TextWrapMode::Extend));
                                });
                        }
                    }

                    if resp.clicked() || resp.dragged() {
                        ui.set_cursor_icon(CursorIcon::Grabbing);
                        self.dragging_seeker = true;
                    }

                    if (resp.clicked() || !resp.dragged()) && self.dragging_seeker && let Some(p) = dragging_progress {
                        self.player.seek(state.duration().mul_f32(p));
                        self.dragging_seeker = false;
                    }

                    ui.add_space(7.5);
                    ui.columns_const(|columns: &mut [Ui; 3]| {
                        columns[0].horizontal(|ui| {
                            ui.add_space(7.5);
                            let mut art_url = None;
                            if let Some(s) = &song {
                                if let Some(cover_art) = &s.cover_art {
                                    art_url = Some(format!("https://images.neurokaraoke.com/WxURxyML82UkE7gY-PiBKw/{}/w=70,h=70,fit=cover,quality=90", cover_art.cloudflare_id));
                                }
                            }
                            if art_url.is_none() {
                                if let Ok(meta) = self.player.current_url_metadata.lock() {
                                    if let Some(meta) = &*meta {
                                        art_url = meta.cover_art.as_ref().map(|url| url.to_string());
                                    }
                                }
                            }
                            
                            if let Some(url) = art_url {
                                ui.add(egui::Image::new(url)
                                    .fit_to_exact_size(Vec2::new(70.0, 70.0)).corner_radius(8.0));
                            }

                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                ui.add_space(5.0);
                                let title = song.as_ref().map(|s| s.title.to_string()).unwrap_or_else(|| "Unknown Song".to_string());
                                ui.add(egui::Label::new(RichText::new(title).size(24.0)).wrap_mode(TextWrapMode::Truncate));
                                if let Some(s) = &song {
                                    ui.add(egui::Label::new(RichText::new(format!("{} (feat. {})", s.original_artists.join(" & "), s.cover_artists.join(" & "))).color(self.theme.text_muted).size(12.0)).wrap_mode(TextWrapMode::Truncate));
                                }
                                let position = state.position().as_secs();
                                let duration = state.duration().as_secs();
                                ui.add(egui::Label::new(RichText::new(format!("{}:{:02} / {}:{:02}", position / 60, position % 60, duration / 60, duration % 60)).color(self.theme.text_muted).size(10.0)).wrap_mode(TextWrapMode::Truncate))
                            });
                        });

                        columns[1].horizontal_centered(|ui| {
                            let total_width = 108.0; // 24 + 10 + 40 + 10 + 24
                            let available = ui.available_width();

                            ui.add_space((available - total_width).max(0.0) * 0.5);

                            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

                            fn btn(theme: &ThemeManager, ui: &mut Ui, source: ImageSource, active: bool, set_active: impl FnOnce(bool)) {
                                let resp = ui.add(egui::Image::new(source).fit_to_exact_size(Vec2::new(24.0, 24.0)).tint(if active { theme.accent_light } else { theme.text })).interact(Sense::click());
                                if resp.hovered() {
                                    ui.set_cursor_icon(CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    set_active(!active);
                                }
                            }

                            btn(&self.theme, ui, include_image!("../assets/backward.png"), false, |x| {
                                self.player.previous();
                            });
                            ui.add_space(10.0);

                            btn(&self.theme, ui, include_image!("../assets/shuffle.png"), self.config.shuffle, |x| {
                                self.config.shuffle = x;
                                self.player.shuffle(x);
                            });

                            ui.add_space(10.0);

                            let resp = ui.add(egui::Button::image(egui::Image::new(
                                if state.paused() { include_image!("../assets/play.png") }
                                else { include_image!("../assets/pause.png") }
                            ).fit_to_exact_size(Vec2::new(24.0, 24.0)))
                                .min_size(Vec2::new(40.0, 40.0))
                                .corner_radius(20.0)
                                .fill(self.theme.primary)
                            );

                            if resp.hovered() {
                                ui.set_cursor_icon(CursorIcon::PointingHand);
                            }

                            if resp.clicked() {
                                if state.paused() { self.player.play(); }
                                else { self.player.pause(); }
                            }

                            ui.add_space(10.0);

                            btn(&self.theme, ui, match self.config.loop_mode {
                                LoopMode::One => include_image!("../assets/loop-one.svg"),
                                _ => include_image!("../assets/loop.svg"),
                            }, self.config.loop_mode != LoopMode::None, |_| {
                                let next_mode = match self.config.loop_mode {
                                    LoopMode::None => LoopMode::All,
                                    LoopMode::One => LoopMode::None,
                                    LoopMode::All => LoopMode::One,
                                };
                                crate::debug_log!("Loop mode toggled: {:?} -> {:?}", self.config.loop_mode, next_mode);
                                self.config.loop_mode = next_mode;
                                self.player.looping(next_mode);
                            });


                            ui.add_space(10.0);

                            btn(&self.theme, ui, include_image!("../assets/forward.png"), false, |x| {
                                self.player.next_song();
                            });
                        });

                        columns[2].scope(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                                let mut rect = ui.max_rect();
                                rect.set_right(rect.right() - 5.0);
                                rect.set_left(rect.right() - 5.0);
                                rect.set_top(rect.top() + 5.0);
                                rect.set_bottom(rect.bottom() - 5.0);

                                ui.painter().rect_filled(rect, 0.0, self.theme.background_elevated);
                                let mut mesh = egui::Mesh::default();
                                let lerped_color: Color32 = lerp(Rgba::from(self.theme.accent)..=Rgba::from(self.theme.accent_light), self.config.volume).into();
                                let h = rect.height() * self.config.volume;
                                mesh.colored_vertex(rect.left_bottom(), self.theme.accent);
                                mesh.colored_vertex(rect.left_bottom() - Vec2::new(0.0, h), lerped_color);
                                mesh.colored_vertex(rect.right_bottom() - Vec2::new(0.0, h), lerped_color);
                                mesh.colored_vertex(rect.right_bottom(), self.theme.accent);
                                mesh.add_triangle(0, 1, 2);
                                mesh.add_triangle(0, 2, 3);
                                ui.painter().add(egui::Shape::mesh(mesh));

                                let resp = ui.interact(rect, ui.id().with("volume_slider"), Sense::click_and_drag());

                                if resp.hovered() {
                                    ui.set_cursor_icon(CursorIcon::PointingHand);
                                }

                                if resp.clicked() || resp.dragged() {
                                    ui.set_cursor_icon(CursorIcon::Grabbing);
                                    self.dragging_volume = true;
                                    if let Some(pos) = ui.pointer_latest_pos() {
                                        self.config.volume = 1.0 - ((pos.y - rect.top()).clamp(0.0, rect.height()) / rect.height());
                                    }
                                }

                                if (resp.clicked() || !resp.dragged()) && self.dragging_volume {
                                    self.player.volume(self.config.volume);
                                    self.dragging_volume = false;
                                }

                                ui.add_space(10.0);
                            });
                        });
                    });

                });
        }

        // central panel
        ui.push_id("central_panel", |ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                .show(ui, |ui| {

                    ui.heading(self.activity.as_str());
                    ui.push_id(self.activity.as_str(), |ui| {
                        if self.activity == ActivityType::Home {
                            ui.add(egui::TextEdit::singleline(&mut self.search));

                            ui.horizontal(|ui| {
                                if ui.button("find and play").clicked() {
                                    let search = self.search.to_lowercase();
                                    self.player.song(self.songs.get_map().iter().find(|x| x.value().if_loaded_or_else(|s| s.title.to_lowercase().starts_with(search.as_str()), false)).map(|x| *x.key()), Player::play);
                                }

                                if ui.button("add search results to playlist").clicked() {
                                    let search = self.search.to_lowercase();
                                    let mut pl = self.player.get_playlist().map(|x| Vec::from(&*x)).unwrap_or_else(Vec::new);
                                    pl.append(&mut self.songs.get_map().iter().filter(|x| x.value().if_loaded_or_else(|s| s.title.to_lowercase().starts_with(search.as_str()), false)).map(|x| *x.key()).collect::<Vec<Uuid>>());
                                    self.player.playlist(Some(pl.into()));
                                }

                                if ui.button("delete playlist").clicked() {
                                    self.player.playlist(None);
                                }
                            });

                            if let Some(pl) = self.player.get_playlist() {
                                ui.label("Playlist:");
                                egui::Frame::new().fill(self.theme.background_elevated).show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        for song in &*pl {
                                            if let LoadingState::Loaded(title) = self.songs.get(song, |s| s.title.clone()) {
                                                ui.label(title.to_string());
                                            }
                                        }
                                    });
                                });
                            }
                        } else if self.activity == ActivityType::Playlists {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.label("Public Playlists:");
                                match &*self.playlist_activity.playlists.blocking_lock() {
                                    LoadingState::Loaded(playlists) => {
                                        for playlist in playlists {
                                            if ui.button(format!("{} by {}", playlist.name, playlist.creator)).clicked() {
                                                self.playlist_activity.select_playlist(playlist.id);
                                            }
                                        }
                                    },
                                    LoadingState::Loading => {
                                        ui.label("Loading...");
                                    },
                                    LoadingState::Failed(err) => {
                                        ui.label(format!("Error loading playlists: {}", err));
                                    }
                                }
                                
                                if let Some(selected) = &*self.playlist_activity.selected_playlist.blocking_lock() {
                                    ui.separator();
                                    match selected {
                                        LoadingState::Loaded(detail) => {
                                            ui.label(format!("Playlist: {}", detail.name));
                                            if ui.button("Play Playlist").clicked() {
                                                let songs = &detail.songs;
                                                crate::debug_log!("Playlist '{}' has {} songs.", detail.name, songs.len());
                                                // Restore playlist for Player logic
                                                let pl: Vec<Uuid> = songs.iter().map(|_| Uuid::new_v4()).collect();
                                                self.player.playlist(Some(pl.clone().into()));
                                                self.player.url_playlist(Some(songs.clone().into()));
                                                
                                                if let Some(first_song) = songs.first() {
                                                    self.player.url_playback(Some(pl[0]), first_song.clone(), Player::play);
                                                }
                                            }
                                            for song in &detail.songs {
                                                ui.label(song.title.to_string());
                                            }
                                        },
                                        LoadingState::Loading => {
                                            ui.label("Loading playlist details...");
                                        },
                                        LoadingState::Failed(err) => {
                                            ui.label(format!("Error loading playlist details: {}", err));
                                        }
                                    }
                                }
                            });
                        }
                    });
                });
        });

        if ui.input(|i| i.focused) {
            ui.request_repaint();
        } else if self.config.framerate_when_not_focused > 0.0 {
            ui.request_repaint_after(Duration::from_micros((1_000_000.0 / self.config.framerate_when_not_focused) as u64));
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(e) = self.config.write() {
            eprintln!("config write failed: {}", e);
        }

        let (cache, client, config) = (self.cache.clone(), self.client.clone(), self.config.clone());
        if let Err(e) = self.rt.block_on(self.rt.spawn(async move { cache.cache_pass(client, &config.cache.lock().await.clone()).await; })) {
            eprintln!("cache write failed: {}", e);
        }
    }
}