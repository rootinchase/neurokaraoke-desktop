use crate::api::{Artwork, LoadingState, Song};
use crate::app::state::App;
use crate::audio::{LoopMode, PlaybackState, Player};
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::util::format_duration;
use eframe::egui::{
    self, Align, Align2, Button, Color32, Context, CursorIcon, Image, ImageSource, Key, Layout,
    Order, PopupKind, Pos2, Rgba, RichText, Sense, Stroke, TextWrapMode, Ui, Vec2, include_image,
    lerp,
};
use egui::{
    Area, Frame, Id, InnerResponse, Label, Margin, Mesh, Panel, Popup, PopupAnchor, Rect, Shape,
    vec2,
};
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub fn render_sleep_timer_popup(app: &mut App, ctx: &Context, pos: Pos2) -> InnerResponse<()> {
    Area::new(Id::new("sleep_timer_area"))
        .fixed_pos(pos)
        .show(ctx, |ui| {
            Frame::window(&ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                ui.heading("Sleep Timer");

                let options = [5, 15, 30, 60, 120];
                for &minutes in &options {
                    if ui.button(format!("{} min", minutes)).clicked() {
                        app.sleep_timer_end =
                            Some(Instant::now() + Duration::from_secs(minutes * 60));
                        app.show_timer_menu = false;
                    }
                }

                ui.add_space(5.0);
                ui.label("Custom (min):");

                let custom_id = ui.make_persistent_id("custom_timer_input");
                let mut custom_text: String =
                    ui.data_mut(|d| d.get_temp(custom_id).unwrap_or_default());

                if ui.text_edit_singleline(&mut custom_text).changed() {
                    ui.data_mut(|d| d.insert_temp(custom_id, custom_text.clone()));
                }

                if ui.button("Set Custom").clicked() {
                    if let Ok(minutes) = custom_text.parse::<u64>() {
                        app.sleep_timer_end =
                            Some(Instant::now() + Duration::from_secs(minutes * 60));
                        app.show_timer_menu = false;
                    }
                }

                ui.add_space(5.0);
                ui.label("Shut off at (HH:MM):");

                let time_id = ui.make_persistent_id("custom_time_input");
                let mut time_text: String =
                    ui.data_mut(|d| d.get_temp(time_id).unwrap_or_default());

                if ui.text_edit_singleline(&mut time_text).changed() {
                    ui.data_mut(|d| d.insert_temp(time_id, time_text.clone()));
                }

                if ui.button("Set Time").clicked() {
                    let input = time_text.to_lowercase();
                    let is_pm = input.contains("pm");
                    let is_am = input.contains("am");
                    let time_clean = input.replace("am", "").replace("pm", "").trim().to_string();

                    if let Some((h_str, m_str)) = time_clean.split_once(':') {
                        if let (Ok(h_raw), Ok(m)) = (h_str.parse::<u32>(), m_str.parse::<u32>()) {
                            let mut h = h_raw;

                            if is_pm && h < 12 {
                                h += 12;
                            } else if is_am && h == 12 {
                                h = 0;
                            }

                            if h < 24 && m < 60 {
                                let now = chrono::Local::now();
                                let target = now.date_naive().and_hms_opt(h, m, 0).unwrap();
                                let target_dt = target.and_local_timezone(chrono::Local).unwrap();

                                let target_dt = if target_dt <= now {
                                    target_dt + chrono::Duration::days(1)
                                } else {
                                    target_dt
                                };

                                let duration = target_dt.signed_duration_since(now);

                                app.sleep_timer_end = Some(
                                    Instant::now()
                                        + Duration::from_secs(duration.num_seconds() as u64),
                                );
                                app.show_timer_menu = false;
                            }
                        }
                    }
                }

                ui.separator();

                if app.sleep_timer_end.is_some() {
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
        })
}

/// Resolves the song currently being played — radio now-playing (azuracast or
/// current-state), the lazy song database, or URL metadata as a fallback —
/// along with the radio elapsed/duration used by the seek bar.
fn resolve_current_song(
    app: &App,
    state: &PlaybackState,
    is_radio: bool,
) -> (Option<Song>, u64, u64) {
    let mut az_elapsed = 0u64;
    let mut az_duration = 0u64;

    let song = if is_radio {
        let mut s = None;
        if let Ok(az_guard) = app.radio_activity.azuracast_state.try_lock() {
            if let LoadingState::Loaded(az) = &*az_guard {
                if let Some(np) = az.effective_now_playing() {
                    az_elapsed = np.elapsed_secs();
                    az_duration = np.duration_secs();
                    if let Some(az_song) = &np.song {
                        let song_id_uuid = az_song.song_id_uuid();

                        let art_uuid = song_id_uuid.unwrap_or_else(|| {
                            if let Some(art_url) = &az_song.art {
                                Uuid::new_v5(&Uuid::NAMESPACE_URL, art_url.as_bytes())
                            } else {
                                Uuid::nil()
                            }
                        });

                        let art = if let Some(art_url) = &az_song.art {
                            Artwork::from_url(art_uuid, art_url)
                        } else {
                            Artwork::default_art()
                        };
                        s = Some(Song {
                            id: Uuid::nil(),
                            title: az_song
                                .title
                                .clone()
                                .unwrap_or_else(|| "Radio Stream".into())
                                .into(),
                            absolute_path: None,
                            opus: None,
                            cover_artists: Arc::from([]),
                            original_artists: az_song
                                .artist
                                .clone()
                                .map(|a| Arc::from([a.into()]))
                                .unwrap_or_else(|| Arc::from([])),
                            cover_art: Some(art),
                            play_count: None,
                            duration: np.duration,
                        });
                    }
                }
            }
        }
        if s.is_none() {
            if let Ok(guard) = app.radio_activity.current_state.try_lock() {
                if let LoadingState::Loaded(curr) = &*guard {
                    if let Some(cur_song) = &curr.current {
                        s = Some(Song {
                            id: Uuid::nil(),
                            title: cur_song.title.clone(),
                            absolute_path: cur_song.audio_url.clone().map(|x| x.to_string().into()),
                            opus: None,
                            cover_artists: cur_song.cover_artists.clone(),
                            original_artists: cur_song.original_artists.clone(),
                            cover_art: cur_song.cover_art.clone(),
                            play_count: None,
                            duration: cur_song.duration,
                        });
                    }
                }
            }
        }
        if s.is_none() {
            s = Some(Song {
                id: Uuid::nil(),
                title: "24/7 NeuroKaraoke Radio".into(),
                absolute_path: None,
                opus: None,
                cover_artists: Arc::from([]),
                original_artists: Arc::from([]),
                cover_art: Some(Artwork::default_art()),
                play_count: None,
                duration: None,
            });
        }
        s
    } else {
        let mut s = match app.songs.get(&state.song(), |song| song.clone()) {
            LoadingState::Loaded(s) => Some(s),
            _ => None,
        };

        // Fallback to URL-based metadata if database lookup failed
        if s.is_none() {
            if let Ok(meta) = app.player.current_url_metadata.lock() {
                if let Some(meta) = &*meta
                    && meta.id == state.song()
                {
                    s = Some(Song {
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
        s
    };

    (song, az_elapsed, az_duration)
}

/// Resolves the artwork identifiers (cloudflare id + absolute path) for the
/// current song, falling back to URL metadata when the song has no cover art.
fn current_artwork_refs(
    app: &App,
    song: Option<&Song>,
    state: &PlaybackState,
) -> (Option<Arc<str>>, Option<Arc<str>>) {
    let mut current_img_uuid: Option<Arc<str>> = None;
    let mut current_abs_path: Option<Arc<str>> = None;
    if let Some(s) = song {
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
    (current_img_uuid, current_abs_path)
}

pub fn render_player_controls(app: &mut App, ui: &mut Ui) {
    let player_vol = app.player.get_volume();
    if (app.config.volume - player_vol).abs() > f32::EPSILON {
        app.config.volume = player_vol;
        let _ = app.config.write();
    }

    if let Some(state) = app.player.get_playback_state() {
        let is_radio = state.song() == Uuid::nil();
        let (song, az_elapsed, az_duration) = resolve_current_song(app, &state, is_radio);

        Panel::bottom("player")
            .resizable(false)
            .frame(
                Frame::new()
                    .inner_margin(Margin {
                        left: 0,
                        right: 0,
                        top: 2,
                        bottom: 2,
                    })
                    .outer_margin(0.0)
                    .stroke(Stroke::new(0.0, Color32::TRANSPARENT))
                    .fill(app.theme.background_secondary),
            )
            .show(ui, |ui| {
                // progress bar
                let mut rect = ui.max_rect();
                rect.set_height(3.0);
                let dragging_progress = ui
                    .pointer_latest_pos()
                    .map(|pos| (pos.x - rect.left()).clamp(0.0, rect.width()) / rect.width());
                let (position_secs, duration_secs, progress) = if is_radio {
                    let p = az_elapsed as f64;
                    let d = if az_duration > 0 {
                        az_duration as f64
                    } else {
                        1.0
                    };
                    (az_elapsed, az_duration, (p / d) as f32)
                } else {
                    let pos = if app.dragging_seeker
                        && let Some(p) = dragging_progress
                    {
                        state.duration().mul_f32(p)
                    } else {
                        state.position()
                    };
                    let dur = state.duration();
                    let prog = if dur.as_millis() > 0 {
                        (pos.as_millis() as f64 / dur.as_millis() as f64) as f32
                    } else {
                        0.0
                    };
                    (pos.as_secs(), dur.as_secs(), prog)
                };

                ui.painter()
                    .rect_filled(rect, 0.0, app.theme.background_elevated);
                let mut mesh = Mesh::default();
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
                ui.painter().add(Shape::mesh(mesh));

                ui.advance_cursor_after_rect(rect);

                if !is_radio {
                    let resp = ui.interact(rect, ui.id().with("seeker"), Sense::click_and_drag());

                    if resp.hovered() || resp.dragged() {
                        ui.set_cursor_icon(CursorIcon::PointingHand);
                        if let Some(pos) = ui.pointer_latest_pos()
                            && let Some(p) = dragging_progress
                        {
                            Popup::new(
                                ui.id().with("seeker_tooltip"),
                                ui.ctx().clone(),
                                PopupAnchor::Position(Pos2::new(
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
                                    Label::new(RichText::new(format_duration(point)).size(12.0))
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
                }

                ui.horizontal(|ui| {
                    let total_width = ui.available_width();
                    let center_width = if is_radio { 64.0 } else { 240.0 };
                    let right_width = 160.0;

                    // Left side (Cover + Metadata) rendered first to measure its natural width
                    let left_resp = ui
                        .horizontal(|ui| {
                            ui.add_space(7.5);
                            let (current_img_uuid, current_abs_path) =
                                current_artwork_refs(app, song.as_ref(), &state);

                            let mut cached_path_str = None;
                            if let Some(abs_path) = current_abs_path {
                                cached_path_str =
                                    app.resolve_artwork_uri(ui.ctx(), current_img_uuid, abs_path);
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
                                            .fit_to_exact_size(vec2(size, size))
                                            .bg_fill(Color32::TRANSPARENT)
                                            .corner_radius(8.0),
                                    );
                                }
                            } else {
                                let (rect, _) =
                                    ui.allocate_exact_size(Vec2::new(size, size), Sense::hover());
                                ui.painter()
                                    .rect_filled(rect, 8.0, app.theme.background_elevated);
                            }

                            ui.add_space(8.0);

                            //Song metadata display
                            ui.scope(|ui| {
                                ui.set_max_width(280.0);
                                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                                    ui.add_space(1.0);
                                    let title = song
                                        .as_ref()
                                        .map(|s| s.title.to_string())
                                        .unwrap_or_else(|| "Unknown Song".to_string());
                                    ui.add(
                                        Label::new(RichText::new(title).size(22.0))
                                            .wrap_mode(TextWrapMode::Truncate),
                                    );
                                    if let Some(s) = &song {
                                        ui.add(
                                            Label::new(
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
                                    ui.add(
                                        Label::new(
                                            RichText::new(format!(
                                                "{}{}:{:02} / {}:{:02}",
                                                if is_radio { "🔴 " } else { "" },
                                                position_secs / 60,
                                                position_secs % 60,
                                                duration_secs / 60,
                                                duration_secs % 60
                                            ))
                                            .color(if is_radio {
                                                app.theme.primary
                                            } else {
                                                app.theme.text_muted
                                            })
                                            .size(if is_radio { 11.0 } else { 10.0 }),
                                        )
                                        .wrap_mode(TextWrapMode::Truncate),
                                    );
                                });
                            });
                        })
                        .response;

                    let left_width = left_resp.rect.width();
                    let remaining =
                        (total_width - left_width - center_width - right_width).max(0.0);
                    let spacer = remaining / 2.0;

                    ui.add_space(spacer);

                    // Center control
                    ui.allocate_ui(vec2(center_width, ui.available_height()), |ui| {
                        ui.with_layout(
                            Layout::left_to_right(Align::Center).with_main_align(Align::Center),
                            |ui| {
                                if !is_radio {
                                    app.config.shuffle = app.shared_config.shuffle.load(SeqCst);
                                    let current_mode_u32 = app.shared_config.loop_mode.load(SeqCst);
                                    app.config.loop_mode = match current_mode_u32 {
                                        1 => LoopMode::One,
                                        2 => LoopMode::All,
                                        _ => LoopMode::None,
                                    };

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
                                        app.shared_config.shuffle.store(app.config.shuffle, SeqCst);
                                        let _ = app.config.write();
                                        ui.ctx().request_repaint();
                                    }

                                    ui.add_space(10.0);
                                }

                                // Play/Pause
                                let resp = ui.add(
                                    Button::image(
                                        Image::new(if state.paused() {
                                            include_image!("../../assets/play.svg")
                                        } else if is_radio {
                                            include_image!("../../assets/stop.svg")
                                        } else {
                                            include_image!("../../assets/pause.svg")
                                        })
                                        .fit_to_exact_size(if !is_radio {
                                            Vec2::new(24.0, 24.0)
                                        } else if !state.paused() {
                                            Vec2::new(16.0, 16.0)
                                        } else {
                                            Vec2::new(24.0, 24.0)
                                        }),
                                    )
                                    .min_size(Vec2::new(40.0, 40.0))
                                    .corner_radius(20.0)
                                    .fill(app.theme.primary),
                                );

                                if resp.hovered() {
                                    ui.set_cursor_icon(CursorIcon::PointingHand);
                                }

                                if resp.clicked() {
                                    if is_radio {
                                        if state.paused() {
                                            app.player.radio_stream(
                                                app.config.radio_url.url().to_string(),
                                                Player::play,
                                            );
                                        } else {
                                            app.player.pause();
                                        }
                                    } else {
                                        if state.paused() {
                                            app.player.play();
                                        } else {
                                            app.player.pause();
                                        }
                                    }
                                }

                                if !is_radio {
                                    ui.add_space(10.0);

                                    //Loop
                                    if btn(
                                        &app.theme,
                                        ui,
                                        match app.config.loop_mode {
                                            LoopMode::One => {
                                                include_image!("../../assets/loop-one.svg")
                                            }
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
                                        app.shared_config.loop_mode.store(mode_u32, SeqCst);

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
                                }
                            },
                        );
                    });

                    ui.add_space(spacer);

                    // Right Side controls
                    ui.allocate_ui(vec2(right_width, ui.available_height()), |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // Volume control progress bar
                                //ui.add_space(8.0); // Give a little margin from the favorite/timer icons

                                let desired_size = vec2(16.0, ui.available_height() - 5.0);
                                let (rect, resp) =
                                    ui.allocate_exact_size(desired_size, Sense::click_and_drag());

                                let progress = app.config.volume.clamp(0.0, 1.0);

                                if resp.hovered() || resp.dragged() {
                                    ui.set_cursor_icon(CursorIcon::PointingHand);
                                }

                                if resp.dragged() || resp.clicked() {
                                    if let Some(pos) = ui.pointer_latest_pos() {
                                        let p = ((rect.bottom() - pos.y) / rect.height())
                                            .clamp(0.0, 1.0);
                                        if (app.config.volume - p).abs() > f32::EPSILON {
                                            app.config.volume = p;
                                            app.player.volume(app.config.volume);
                                            app.shared_config
                                                ._volume
                                                .store(app.config.volume.to_bits(), SeqCst);
                                            let _ = app.config.write();
                                            ui.ctx().request_repaint();
                                        }
                                    }
                                }

                                if resp.hovered() || resp.dragged() {
                                    if let Some(pos) = ui.pointer_latest_pos() {
                                        Popup::new(
                                            ui.id().with("volume_tooltip"),
                                            ui.ctx().clone(),
                                            PopupAnchor::Position(Pos2::new(
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
                                                Label::new(
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
                                let track_rect =
                                    Rect::from_center_size(rect.center(), vec2(6.0, rect.height()));
                                ui.painter().rect_filled(
                                    track_rect,
                                    3.0,
                                    app.theme.background_elevated,
                                );

                                // Draw filled-in part using theme accent
                                let filled_height = rect.height() * progress;
                                let filled_rect = Rect::from_min_max(
                                    Pos2::new(track_rect.left(), rect.bottom() - filled_height),
                                    Pos2::new(track_rect.right(), rect.bottom()),
                                );
                                ui.painter()
                                    .rect_filled(filled_rect, 3.0, app.theme.primary);

                                ui.add_space(10.0);

                                // Favorite button - conditional on song
                                // NOTE: This block is intentionally restricted to songs.
                                // Do not allow radio or other media types here, as they cannot be favorited.
                                if !is_radio {
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
                                    timer_btn_resp = timer_btn_resp.on_hover_text(format!(
                                        "Sleep Timer: {} remaining",
                                        time_str
                                    ));
                                    ui.ctx().request_repaint();
                                } else {
                                    timer_btn_resp = timer_btn_resp.on_hover_text("Sleep Timer");
                                }

                                if timer_btn_resp.clicked() {
                                    app.show_timer_menu = !app.show_timer_menu;
                                }

                                ui.add_space(10.0);

                                // Queue button
                                if !is_radio {
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
                                }

                                ui.add_space(10.0);

                                // Fullscreen toggle button
                                let fullscreen_source = if app.show_fullscreen {
                                    include_image!("../../assets/fullscreen-exit.svg")
                                } else {
                                    include_image!("../../assets/fullscreen.svg")
                                };
                                if btn(&app.theme, ui, fullscreen_source, app.show_fullscreen) {
                                    app.show_fullscreen = !app.show_fullscreen;
                                    ui.ctx().request_repaint();
                                }

                                // Timer menu popup logic
                                if app.show_timer_menu {
                                    // Use the measured popup height from the previous frame
                                    // so its bottom edge sits flush with the top of the panel.
                                    let height_id =
                                        ui.make_persistent_id("sleep_timer_popup_height");
                                    let popup_height: f32 =
                                        ui.data(|d| d.get_temp(height_id).unwrap_or(300.0));
                                    // The seek bar occupies the top 3px of the panel, so
                                    // offset the popup above it (seek bar height + margin).
                                    let seek_bar_height = 3.0;
                                    let margin = 7.0;
                                    let panel_top = ui.max_rect().top();
                                    let pos = Pos2::new(
                                        timer_btn_resp.rect.center().x,
                                        panel_top - popup_height - seek_bar_height - margin,
                                    );

                                    let popup_resp = render_sleep_timer_popup(app, ui.ctx(), pos);

                                    // Store the measured height for the next frame.
                                    ui.data_mut(|d| {
                                        d.insert_temp(height_id, popup_resp.response.rect.height())
                                    });
                                }

                                ui.add_space(10.0);
                            });
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

/// Draws the fullscreen player overlay: full-resolution artwork filling the
/// screen above the embedded player controls. Esc exits, and the toggle button
/// in the player controls (now showing the collapse icon) also exits.
pub fn render_fullscreen_player(app: &mut App, ctx: &Context) {
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.show_fullscreen = false;
        ctx.request_repaint();
        return;
    }

    let screen_rect = ctx.input(|i| i.content_rect());

    Area::new(Id::new("fullscreen_player"))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .default_size(screen_rect.size())
        .constrain(false)
        .order(Order::Foreground)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen_rect, 0.0, app.theme.background);

            // The player controls dock to the bottom of the overlay, giving the
            // fullscreen view embedded play/pause, seek, volume, favorite, timer and queue.
            render_player_controls(app, ui);

            let avail = ui.available_rect_before_wrap();
            let size = avail.width().min(avail.height());
            if size <= 0.0 {
                return;
            }

            let (rect, _resp) =
                ui.allocate_exact_size(vec2(avail.width(), avail.height()), Sense::hover());

            let artwork_path = app.player.get_playback_state().and_then(|state| {
                let is_radio = state.song() == Uuid::nil();
                let (song, _, _) = resolve_current_song(app, &state, is_radio);
                let (img_uuid, abs_path) = current_artwork_refs(app, song.as_ref(), &state);
                app.resolve_full_artwork_uri(
                    ctx,
                    img_uuid,
                    abs_path.unwrap_or_else(|| Arc::from("")),
                )
            });

            let art_rect = Rect::from_center_size(rect.center(), vec2(size, size));

            if let Some(path_str) = artwork_path {
                if let Ok(image_bytes) = std::fs::read(&path_str) {
                    let image_source = ImageSource::Bytes {
                        uri: std::borrow::Cow::Owned(format!("bytes://{}", path_str)),
                        bytes: image_bytes.into(),
                    };
                    ui.put(
                        art_rect,
                        Image::new(image_source)
                            .max_size(vec2(size, size))
                            .bg_fill(Color32::TRANSPARENT)
                            .corner_radius(20.0),
                    );
                }
            } else {
                ui.painter()
                    .rect_filled(art_rect, 20.0, app.theme.background_elevated);
            }
        });
}
