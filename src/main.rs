mod theme;
mod activity;
mod audio;

use crate::activity::home::HomeActivity;
use crate::activity::ActivityType;
use crate::audio::Player;
use crate::theme::{SelectableTheme, Theme, ThemeManager};
use eframe::egui::{include_image, lerp, Align, Color32, CornerRadius, CursorIcon, ImageSource, Layout, PopupKind, Pos2, RectAlign, Rgba, RichText, Sense, Stroke, TextWrapMode, Tooltip, Ui, Vec2};
use eframe::{egui, Frame};
use neurokaraoke_metadata_client::{Artwork, AsUuid, Database, DatabaseBuilder, Song};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

fn main() -> eframe::Result<()> {
    let runtime = Arc::new(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap());

    let _guard = runtime.enter();

    App::run(runtime)
}

struct App {
    pub rt: Arc<tokio::runtime::Runtime>,

    pub songs: Arc<Database<Song>>,
    pub artwork: Arc<Database<Artwork>>,

    pub player: Player,

    pub search: String,
    pub dragging_seeker: bool,
    pub dragging_volume: bool,
    pub volume: f32,
    pub shuffle: bool,
    pub looping: bool,

    // theme stuff
    pub theme: ThemeManager,
    pub theme_selector: SelectableTheme,

    // activity stuff
    pub activity: ActivityType,
    pub home_activity: HomeActivity,
}

impl App {
    fn new(ctx: &egui::Context, rt: Arc<tokio::runtime::Runtime>) -> Self {
        egui_extras::install_image_loaders(ctx);

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

        let songs = Arc::new(rt.block_on(DatabaseBuilder::songs()/*.cache_file("./songcache.msgpack.zst")*/.zstd().msgpack().build()).unwrap());
        let artwork = Arc::new(rt.block_on(DatabaseBuilder::art()/*.cache_file("./artcache.msgpack.zst")*/.zstd().msgpack().build()).unwrap());

        let player = Player::new(rt.clone(), ctx.clone(), songs.clone());

        Self {
            songs,
            artwork: artwork.clone(),

            player,

            search: "".to_string(),
            dragging_seeker: false,
            dragging_volume: false,
            volume: 1.0,
            shuffle: false,
            looping: false,

            theme: ThemeManager::new(Theme::neuro()),
            theme_selector: SelectableTheme::Neuro,

            activity: ActivityType::Home,
            home_activity: HomeActivity::new(ctx.clone()),

            rt,
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
            Box::new(|cc| Ok(Box::new(Self::new(&cc.egui_ctx, rt)))),
        )
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
                        |ui| ui.label(RichText::new(self.theme_selector.karaoke_str()).color(self.theme.primary_dark).size(24.0))
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

                // bottom area
                ui.with_layout(
                    egui::Layout::bottom_up(egui::Align::LEFT),
                    |ui| {
                        ui.add_space(10.0);

                        // theme switcher
                        ui.horizontal(|ui| {
                            let current = self.theme_selector;
                            let mut button = |ui: &mut Ui, select_theme: SelectableTheme| {
                                let mut button = egui::Button::new(select_theme.as_str())
                                    .fill(if current == select_theme { self.theme.primary } else { self.theme.background_elevated });

                                button = match select_theme {
                                    SelectableTheme::Neuro => button.corner_radius(CornerRadius { nw: 5, sw: 5, ..Default::default() }),
                                    SelectableTheme::Evil => button.corner_radius(CornerRadius { ne: 5, se: 5, ..Default::default() }),
                                    _ => button.corner_radius(0),
                                };

                                if ui.add_sized([ui.available_width(), 0.0], button).clicked() {
                                    self.theme_selector = select_theme;
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

                            if current != self.theme_selector {
                                self.theme.set(self.theme_selector.as_theme());
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
                    let song = &self.songs[state.song()];
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
                        if let Some(pos) = ui.pointer_latest_pos() && let Some(p) = dragging_progress { egui::Popup::new(
                            ui.id().with("seeker_tooltip"),
                            ui.ctx().clone(),
                            egui::PopupAnchor::Position(Pos2::new(pos.x.clamp(rect.left(), rect.right()), rect.top() - 5.0)),
                            ui.layer_id(),
                        ).align(RectAlign::TOP).kind(PopupKind::Tooltip).open(true).show(|ui| {
                            let point = state.duration().mul_f32(p).as_secs();
                            ui.label(format!("{}:{:02}", point / 60, point % 60));
                        }); }
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
                            ui.add(egui::Image::new(
                                self.artwork[song.cover_art_uuid.unwrap_or_else(|| "68441d52-a231-4c0d-a221-92e1b52ace2e".parse().unwrap())].cloudflare_url.as_str().to_string() +
                                    "w=70,h=70,fit=cover,quality=90"
                            ).fit_to_exact_size(Vec2::new(70.0, 70.0)).corner_radius(8.0));

                            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                ui.add_space(5.0);
                                ui.add(egui::Label::new(RichText::new(format!("{}", song.title)).size(24.0)).wrap_mode(TextWrapMode::Truncate));
                                ui.add(egui::Label::new(RichText::new(format!("{} (feat. {})", song.artists.join(" & "), song.covered_by.join(" & "))).color(self.theme.text_muted).size(12.0)).wrap_mode(TextWrapMode::Truncate));
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

                            btn(&self.theme, ui, include_image!("../assets/shuffle.png"), self.shuffle, |x| {
                                self.shuffle = x;
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

                            btn(&self.theme, ui, include_image!("../assets/loop.png"), self.looping, |x| {
                                self.looping = x;
                                self.player.looping(x);
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
                                let lerped_color: Color32 = lerp(Rgba::from(self.theme.accent)..=Rgba::from(self.theme.accent_light), self.volume).into();
                                let h = rect.height() * self.volume;
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
                                        self.volume = 1.0 - ((pos.y - rect.top()).clamp(0.0, rect.height()) / rect.height());
                                    }
                                }

                                if (resp.clicked() || !resp.dragged()) && self.dragging_volume {
                                    self.player.volume(self.volume.powi(3));
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
                                    self.player.song(self.songs.iter().find(|x| x.title.to_lowercase().starts_with(search.as_str())).map(|x| x.uuid), Player::play);
                                }

                                if ui.button("add search results to playlist").clicked() {
                                    let search = self.search.to_lowercase();
                                    let mut pl = self.player.get_playlist().map(|x| Vec::from(&*x)).unwrap_or_else(Vec::new);
                                    pl.append(&mut self.songs.iter().filter(|x| x.title.to_lowercase().starts_with(search.as_str())).map(|x| x.uuid()).collect::<Vec<Uuid>>());
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
                                            ui.label(self.songs[*song].title.clone());
                                        }
                                    });
                                });
                            }
                        }
                    });
                });
        });

        // power saving while still doing rapid redraws
        if ui.input(|i| i.focused) {
            ui.request_repaint();
        } else {
            ui.request_repaint_after(Duration::from_millis(500));
        }
    }
}