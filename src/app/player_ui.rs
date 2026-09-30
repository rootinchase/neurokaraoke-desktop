use crate::api::Song;
use crate::app::state::App;
use crate::audio::LoopMode;
use crate::debug_log;
use crate::theme::ThemeManager;
use eframe::egui::{
    self, Align, Button, Color32, CursorIcon, Image, ImageSource, Layout, PopupKind, Pos2, Rgba,
    RichText, Sense, Stroke, TextWrapMode, Ui, Vec2, include_image, lerp,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn render_player_controls(app: &mut App, ui: &mut Ui) {
    let player_vol = app.player.get_volume();
    if (app.config.volume - player_vol).abs() > f32::EPSILON {
        app.config.volume = player_vol;
    }

    if let Some(state) = app.player.get_playback_state() {
        let mut song = match app.songs.get(&state.song(), |song| song.clone()) {
            crate::api::LoadingState::Loaded(s) => Some(s),
            _ => None,
        };

        // Fallback to URL-based metadata if database lookup failed
        if song.is_none() {
            if let Ok(meta) = app.player.current_url_metadata.lock() {
                if let Some(meta) = &*meta
                    && meta.id == state.song()
                {
                    song = Some(Song {
                        id: state.song(),
                        title: meta.title.clone(),
                        absolute_path: meta.audio_url.clone().map(|s| s.to_string().into()),
                        opus: None,
                        cover_artists: meta.cover_artists.clone(),
                        original_artists: meta.original_artists.clone(),
                        cover_art: meta.cover_art.clone(),
                        play_count: None,
                        duration: None,
                    });
                }
            }
        }

        egui::Panel::bottom("player")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 2,
                        bottom: 2,
                    })
                    .outer_margin(0.0)
                    .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                    .fill(app.theme.background_secondary),
            )
            //.max_size(90.0)
            .show(ui, |ui| {
                //ui.set_max_height(85.0);
                // progress bar
                let mut rect = ui.max_rect();
                rect.set_height(3.0);
                let dragging_progress = ui
                    .pointer_latest_pos()
                    .map(|pos| (pos.x - rect.left()).clamp(0.0, rect.width()) / rect.width());
                let position = if app.dragging_seeker
                    && let Some(p) = dragging_progress
                {
                    state.duration().mul_f32(p)
                } else {
                    state.position()
                };
                let progress =
                    (position.as_millis() as f64 / state.duration().as_millis() as f64) as f32;
                ui.painter()
                    .rect_filled(rect, 0.0, app.theme.background_elevated);
                let mut mesh = egui::Mesh::default();
                let lerped_color: Color32 = lerp(
                    Rgba::from(app.theme.accent)..=Rgba::from(app.theme.primary),
                    progress,
                )
                .into();

                let w = rect.width() * progress;
                mesh.colored_vertex(rect.left_top() + Vec2::new(0.0, 1.0), app.theme.accent);
                mesh.colored_vertex(rect.left_top() + Vec2::new(w, 1.0), lerped_color);
                mesh.colored_vertex(rect.left_top() + Vec2::new(w, 3.0), lerped_color);
                mesh.colored_vertex(rect.left_top() + Vec2::new(0.0, 3.0), app.theme.accent);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().add(egui::Shape::mesh(mesh));

                ui.advance_cursor_after_rect(rect);

                let resp = ui.interact(rect, ui.id().with("seeker"), Sense::click_and_drag());

                if resp.hovered() || resp.dragged() {
                    ui.set_cursor_icon(CursorIcon::PointingHand);
                    if let Some(pos) = ui.pointer_latest_pos()
                        && let Some(p) = dragging_progress
                    {
                        egui::Popup::new(
                            ui.id().with("seeker_tooltip"),
                            ui.ctx().clone(),
                            egui::PopupAnchor::Position(Pos2::new(
                                pos.x.clamp(rect.left(), rect.right()),
                                rect.top() - 5.0,
                            )),
                            ui.layer_id(),
                        )
                        .align(Default::default())
                        .kind(PopupKind::Tooltip)
                        .open(true)
                        .show(|ui| {
                            let point = state.duration().mul_f32(p).as_secs();
                            ui.add(
                                egui::Label::new(
                                    RichText::new(crate::utilities::util::format_duration(point))
                                        .size(12.0),
                                )
                                .wrap_mode(TextWrapMode::Extend),
                            );
                        });
                    }
                }

                if resp.clicked() || resp.dragged() {
                    ui.set_cursor_icon(CursorIcon::Grabbing);
                    app.dragging_seeker = true;
                }

                if (resp.clicked() || !resp.dragged())
                    && app.dragging_seeker
                    && let Some(p) = dragging_progress
                {
                    // Verify we are still on the same song before applying the seek
                    if app.current_song_uuid == Some(state.song()) {
                        app.player.seek(state.duration().mul_f32(p));
                    }
                    app.dragging_seeker = false;
                }

                ui.columns_const(|columns: &mut [Ui; 3]| {
                    //columns[0].set_max_height(60.0);
                    columns[0].horizontal(|ui| {
                        //ui.set_height(80.0);
                        ui.add_space(7.5);

                        // --- WRAP HERE TO ENFORCE HEIGHT ---
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), ui.available_height()),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let mut current_img_uuid: Option<Arc<str>> = None;
                                let mut current_abs_path: Option<Arc<str>> = None;
                                if let Some(s) = &song {
                                    if let Some(cover_art) = &s.cover_art {
                                        current_img_uuid = cover_art.cloudflare_id.clone();
                                        current_abs_path = Some(cover_art.absolute_path.clone());
                                    }
                                }
                                if current_img_uuid.is_none() {
                                    if let Ok(meta) = app.player.current_url_metadata.lock() {
                                        if let Some(meta) = &*meta
                                            && meta.id == state.song()
                                        {
                                            if let Some(art) = &meta.cover_art {
                                                current_img_uuid = art.cloudflare_id.clone();
                                                current_abs_path = Some(art.absolute_path.clone());
                                            }
                                        }
                                    }
                                }

                                let mut cached_path_str = None;
                                if let Some(abs_path) = current_abs_path {
                                    cached_path_str = app.resolve_artwork_uri(
                                        ui.ctx(),
                                        current_img_uuid,
                                        abs_path,
                                    );
                                }

                                let size = 80.0;
                                if let Some(path_str) = cached_path_str {
                                    if let Ok(image_bytes) = std::fs::read(&path_str) {
                                        let image_source = ImageSource::Bytes {
                                            uri: std::borrow::Cow::Owned(format!(
                                                "bytes://{}",
                                                path_str
                                            )),
                                            bytes: image_bytes.into(),
                                        };

                                        ui.add(
                                            Image::new(image_source)
                                                .fit_to_exact_size(egui::vec2(size, size))
                                                .bg_fill(Color32::TRANSPARENT)
                                                .corner_radius(8.0),
                                        );
                                    }
                                } else {
                                    let (rect, _) = ui
                                        .allocate_exact_size(Vec2::new(size, size), Sense::hover());
                                    ui.painter().rect_filled(
                                        rect,
                                        8.0,
                                        app.theme.background_elevated,
                                    );
                                }

                                //Song metadata display
                                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                    ui.add_space(1.0);
                                    let title = song
                                        .as_ref()
                                        .map(|s| s.title.to_string())
                                        .unwrap_or_else(|| "Unknown Song".to_string());
                                    ui.add(
                                        egui::Label::new(RichText::new(title).size(22.0))
                                            .wrap_mode(TextWrapMode::Truncate),
                                    );
                                    if let Some(s) = &song {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{} (feat. {})",
                                                    s.original_artists.join(" & "),
                                                    s.cover_artists.join(" & ")
                                                ))
                                                .color(app.theme.text_muted)
                                                .size(12.0),
                                            )
                                            .wrap_mode(TextWrapMode::Truncate),
                                        );
                                    }
                                    let position = state.position().as_secs();
                                    let duration = state.duration().as_secs();
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "{}:{:02} / {}:{:02}",
                                                position / 60,
                                                position % 60,
                                                duration / 60,
                                                duration % 60
                                            ))
                                            .color(app.theme.text_muted)
                                            .size(10.0),
                                        )
                                        .wrap_mode(TextWrapMode::Truncate),
                                    )
                                });
                            },
                        );
                    });

                    // Center control
                    //columns[1].set_max_height(60.0);
                    columns[1].horizontal_centered(|ui| {
                        app.config.shuffle = app
                            .shared_config
                            .shuffle
                            .load(std::sync::atomic::Ordering::SeqCst);
                        let current_mode_u32 = app
                            .shared_config
                            .loop_mode
                            .load(std::sync::atomic::Ordering::SeqCst);
                        app.config.loop_mode = match current_mode_u32 {
                            1 => LoopMode::One,
                            2 => LoopMode::All,
                            _ => LoopMode::None,
                        };

                        //ui.set_height(80.0);
                        if btn(
                            &app.theme,
                            ui,
                            include_image!("../../assets/backward.svg"),
                            false,
                        ) {
                            app.player.previous();
                        }

                        ui.add_space(10.0);

                        if btn(
                            &app.theme,
                            ui,
                            include_image!("../../assets/shuffle.svg"),
                            app.config.shuffle,
                        ) {
                            app.config.shuffle = !app.config.shuffle;
                            app.player.shuffle(app.config.shuffle);
                            app.shared_config
                                .shuffle
                                .store(app.config.shuffle, std::sync::atomic::Ordering::SeqCst);
                            let _ = app.config.write();
                            ui.ctx().request_repaint();
                        }

                        ui.add_space(10.0);

                        // Play/Pause
                        let resp = ui.add(
                            Button::image(
                                Image::new(if state.paused() {
                                    include_image!("../../assets/play.svg")
                                } else {
                                    include_image!("../../assets/pause.svg")
                                })
                                .fit_to_exact_size(Vec2::new(24.0, 24.0)),
                            )
                            .min_size(Vec2::new(40.0, 40.0))
                            .corner_radius(20.0)
                            .fill(app.theme.primary),
                        );

                        if resp.hovered() {
                            ui.set_cursor_icon(CursorIcon::PointingHand);
                        }

                        if resp.clicked() {
                            if state.paused() {
                                app.player.play();
                            } else {
                                app.player.pause();
                            }
                        }

                        ui.add_space(10.0);

                        //Loop
                        if btn(
                            &app.theme,
                            ui,
                            match app.config.loop_mode {
                                LoopMode::One => include_image!("../../assets/loop-one.svg"),
                                _ => include_image!("../../assets/loop.svg"),
                            },
                            app.config.loop_mode != LoopMode::None,
                        ) {
                            let next_mode = match app.config.loop_mode {
                                LoopMode::None => LoopMode::One,
                                LoopMode::One => LoopMode::All,
                                LoopMode::All => LoopMode::None,
                            };
                            debug_log!(
                                "Loop mode toggled: {:?} -> {:?}",
                                app.config.loop_mode,
                                next_mode
                            );
                            app.config.loop_mode = next_mode;
                            app.player.looping(next_mode);

                            let mode_u32 = match next_mode {
                                LoopMode::None => 0,
                                LoopMode::One => 1,
                                LoopMode::All => 2,
                            };
                            app.shared_config
                                .loop_mode
                                .store(mode_u32, std::sync::atomic::Ordering::SeqCst);

                            let _ = app.config.write();
                            ui.ctx().request_repaint();
                        }

                        ui.add_space(10.0);

                        if btn(
                            &app.theme,
                            ui,
                            include_image!("../../assets/forward.svg"),
                            false,
                        ) {
                            app.player.next_song();
                        }
                    });

                    // Right Side controls

                    columns[2].horizontal_centered(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            // Volume control progress bar
                            ui.add_space(8.0); // Give a little margin from the favorite/timer icons

                            let desired_size = egui::vec2(16.0, ui.available_height() - 5.0);
                            let (rect, resp) =
                                ui.allocate_exact_size(desired_size, Sense::click_and_drag());

                            let progress = app.config.volume.clamp(0.0, 1.0);

                            if resp.hovered() || resp.dragged() {
                                ui.set_cursor_icon(CursorIcon::PointingHand);
                            }

                            if resp.dragged() || resp.clicked() {
                                if let Some(pos) = ui.pointer_latest_pos() {
                                    let p =
                                        ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
                                    if (app.config.volume - p).abs() > f32::EPSILON {
                                        app.config.volume = p;
                                        app.player.volume(app.config.volume);
                                        let _ = app.config.write();
                                        ui.ctx().request_repaint();
                                    }
                                }
                            }

                            if resp.hovered() || resp.dragged() {
                                if let Some(pos) = ui.pointer_latest_pos() {
                                    egui::Popup::new(
                                        ui.id().with("volume_tooltip"),
                                        ui.ctx().clone(),
                                        egui::PopupAnchor::Position(Pos2::new(
                                            rect.center().x,
                                            pos.y.clamp(rect.top(), rect.bottom()),
                                        )),
                                        ui.layer_id(),
                                    )
                                    .align(Default::default())
                                    .kind(PopupKind::Tooltip)
                                    .open(true)
                                    .show(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{:.0}%",
                                                    app.config.volume * 100.0
                                                ))
                                                .size(11.0),
                                            )
                                            .wrap_mode(TextWrapMode::Extend),
                                        );
                                    });
                                }
                            }

                            // Draw background track
                            let track_rect = egui::Rect::from_center_size(
                                rect.center(),
                                egui::vec2(6.0, rect.height()),
                            );
                            ui.painter().rect_filled(
                                track_rect,
                                3.0,
                                app.theme.background_elevated,
                            );

                            // Draw filled-in part using theme accent
                            let filled_height = rect.height() * progress;
                            let filled_rect = egui::Rect::from_min_max(
                                Pos2::new(track_rect.left(), rect.bottom() - filled_height),
                                Pos2::new(track_rect.right(), rect.bottom()),
                            );
                            ui.painter()
                                .rect_filled(filled_rect, 3.0, app.theme.primary);

                            ui.add_space(10.0);

                            // Favorite button - conditional on song
                            // NOTE: This block is intentionally restricted to songs.
                            // Do not allow radio or other media types here, as they cannot be favorited.
                            if let Some(state) = app.player.get_playback_state() {
                                let current_song_uuid = state.song();
                                let is_favorite =
                                    app.favorites_activity.is_favorite(&current_song_uuid);
                                let icon = if is_favorite {
                                    include_image!("../../assets/favorite.svg")
                                } else {
                                    include_image!("../../assets/favorite-empty.svg")
                                };

                                let resp = ui
                                    .add(
                                        Image::new(icon)
                                            .fit_to_exact_size(Vec2::new(24.0, 24.0))
                                            .tint(if is_favorite {
                                                app.theme.accent_light
                                            } else {
                                                app.theme.text
                                            }),
                                    )
                                    .interact(Sense::click());

                                if resp.hovered() {
                                    ui.set_cursor_icon(CursorIcon::PointingHand);
                                }

                                if resp.clicked() {
                                    app.favorites_activity
                                        .toggle_favorite(current_song_uuid, &app.songs);
                                }
                                ui.add_space(10.0);
                            }

                            // Always visible buttons
                            let is_timer_active = app.sleep_timer_end.is_some();
                            let image = if is_timer_active {
                                include_image!("../../assets/timer-active.svg")
                            } else {
                                include_image!("../../assets/timer-off.svg")
                            };

                            let image_button = Image::new(image)
                                .fit_to_exact_size(Vec2::new(24.0, 24.0))
                                .tint(if is_timer_active {
                                    app.theme.accent_light
                                } else {
                                    app.theme.text
                                });

                            let mut timer_btn_resp = ui
                                .add(image_button)
                                .interact(Sense::click())
                                .on_hover_cursor(CursorIcon::PointingHand);

                            if let Some(end) = app.sleep_timer_end {
                                let remaining = end.saturating_duration_since(Instant::now());
                                let mins = remaining.as_secs() / 60;
                                let secs = remaining.as_secs() % 60;
                                let time_str = format!("{:02}:{:02}", mins, secs);
                                timer_btn_resp = timer_btn_resp
                                    .on_hover_text(format!("Sleep Timer: {} remaining", time_str));
                                ui.ctx().request_repaint();
                            } else {
                                timer_btn_resp = timer_btn_resp.on_hover_text("Sleep Timer");
                            }

                            if timer_btn_resp.clicked() {
                                app.show_timer_menu = !app.show_timer_menu;
                            }

                            ui.add_space(10.0);

                            // Queue button
                            let image = include_image!("../../assets/player-queue.svg");
                            let image_button = Image::new(image)
                                .fit_to_exact_size(Vec2::new(36.0, 36.0))
                                .tint(if app.show_queue {
                                    app.theme.accent_light
                                } else {
                                    app.theme.text
                                });

                            let queue_btn_resp = ui
                                .add(image_button)
                                .interact(Sense::click())
                                .on_hover_cursor(CursorIcon::PointingHand);

                            if queue_btn_resp.clicked() {
                                app.show_queue = !app.show_queue;
                                ui.ctx().request_repaint();
                            }

                            // Timer menu popup logic
                            if app.show_timer_menu {
                                let pos = timer_btn_resp.rect.left_top() - Vec2::new(0.0, 300.0);
                                egui::Area::new(egui::Id::new("sleep_timer_area"))
                                    .fixed_pos(pos)
                                    .show(ui.ctx(), |ui| {
                                        egui::Frame::window(&ui.style()).show(ui, |ui| {
                                            ui.set_min_width(150.0);
                                            ui.heading("Sleep Timer");
                                            let options = [5, 15, 30, 60, 120];
                                            for &minutes in &options {
                                                if ui.button(format!("{} min", minutes)).clicked() {
                                                    app.sleep_timer_end = Some(
                                                        Instant::now()
                                                            + Duration::from_secs(minutes * 60),
                                                    );
                                                    app.show_timer_menu = false;
                                                }
                                            }
                                            ui.add_space(5.0);
                                            ui.label("Custom (min):");
                                            let custom_id =
                                                ui.make_persistent_id("custom_timer_input");
                                            let mut custom_text: String = ui.data_mut(|d| {
                                                d.get_temp(custom_id).unwrap_or_default()
                                            });
                                            if ui.text_edit_singleline(&mut custom_text).changed() {
                                                ui.data_mut(|d| {
                                                    d.insert_temp(custom_id, custom_text.clone())
                                                });
                                            }
                                            if ui.button("Set Custom").clicked() {
                                                if let Ok(minutes) = custom_text.parse::<u64>() {
                                                    app.sleep_timer_end = Some(
                                                        Instant::now()
                                                            + Duration::from_secs(minutes * 60),
                                                    );
                                                    app.show_timer_menu = false;
                                                }
                                            }
                                            ui.add_space(5.0);
                                            ui.label("Shut off at (HH:MM):");
                                            let time_id =
                                                ui.make_persistent_id("custom_time_input");
                                            let mut time_text: String = ui.data_mut(|d| {
                                                d.get_temp(time_id).unwrap_or_default()
                                            });
                                            if ui.text_edit_singleline(&mut time_text).changed() {
                                                ui.data_mut(|d| {
                                                    d.insert_temp(time_id, time_text.clone())
                                                });
                                            }
                                            if ui.button("Set Time").clicked() {
                                                let input = time_text.to_lowercase();
                                                let is_pm = input.contains("pm");
                                                let is_am = input.contains("am");
                                                let time_clean = input
                                                    .replace("am", "")
                                                    .replace("pm", "")
                                                    .trim()
                                                    .to_string();
                                                if let Some((h_str, m_str)) =
                                                    time_clean.split_once(':')
                                                {
                                                    if let (Ok(h_raw), Ok(m)) =
                                                        (h_str.parse::<u32>(), m_str.parse::<u32>())
                                                    {
                                                        let mut h = h_raw;
                                                        if is_pm && h < 12 {
                                                            h += 12;
                                                        } else if is_am && h == 12 {
                                                            h = 0;
                                                        }
                                                        if h < 24 && m < 60 {
                                                            let now = chrono::Local::now();
                                                            let target = now
                                                                .date_naive()
                                                                .and_hms_opt(h, m, 0)
                                                                .unwrap();
                                                            let target_dt = target
                                                                .and_local_timezone(chrono::Local)
                                                                .unwrap();
                                                            let target_dt = if target_dt <= now {
                                                                target_dt
                                                                    + chrono::Duration::days(1)
                                                            } else {
                                                                target_dt
                                                            };
                                                            let duration = target_dt
                                                                .signed_duration_since(now);
                                                            app.sleep_timer_end = Some(
                                                                Instant::now()
                                                                    + Duration::from_secs(
                                                                        duration.num_seconds()
                                                                            as u64,
                                                                    ),
                                                            );
                                                            app.show_timer_menu = false;
                                                        }
                                                    }
                                                }
                                            }
                                            ui.separator();
                                            if is_timer_active {
                                                if ui.button("Cancel Timer").clicked() {
                                                    app.sleep_timer_end = None;
                                                    app.show_timer_menu = false;
                                                }
                                            }
                                            ui.separator();
                                            if ui.button("Close").clicked() {
                                                app.show_timer_menu = false;
                                            }
                                        });
                                    });
                            }
                            ui.add_space(10.0);
                        });
                    });
                });
            });
    }
}

pub fn btn(theme: &ThemeManager, ui: &mut Ui, source: ImageSource, active: bool) -> bool {
    let resp = ui
        .add(
            Image::new(source)
                .fit_to_exact_size(Vec2::new(24.0, 24.0))
                .tint(if active {
                    theme.accent_light
                } else {
                    theme.text
                }),
        )
        .interact(Sense::click());
    if resp.hovered() {
        ui.set_cursor_icon(CursorIcon::PointingHand);
    }
    resp.clicked()
}
