use eframe::egui::{
    self, Align, CornerRadius, CursorIcon, Layout, RichText, Sense, Ui, Vec2, include_image,
};
use egui::{Button, Image, Label, Panel, TextureOptions};

use crate::activity::{ActivityType, profile::AvatarState, profile::ProfileActivity};
use crate::config::Config;
use crate::theme::{SelectableTheme, ThemeManager};

fn render_avatar(ui: &mut Ui, profile_activity: &ProfileActivity, theme: &ThemeManager) {
    let current_avatar_url = profile_activity
        .state
        .profile_data
        .as_ref()
        .and_then(|p| p.avatar_url.clone());

    if let Some(avatar_url) = current_avatar_url {
        match &profile_activity.state.avatar_state {
            AvatarState::Ready { bytes } => {
                let uri = format!("bytes://avatar_{}.jpeg", avatar_url);

                ui.add(
                    Image::from_bytes(uri, bytes.clone())
                        .fit_to_exact_size(Vec2::new(32.0, 32.0))
                        .corner_radius(16.0)
                        .texture_options(TextureOptions::LINEAR),
                );
            }
            AvatarState::Downloading | AvatarState::None => {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 32.0), Sense::hover());
                ui.painter()
                    .rect_filled(rect, 16.0, theme.background_elevated);
            }
        }
    } else {
        ui.add(
            Image::new(include_image!("../../assets/icon.png"))
                .fit_to_exact_size(Vec2::new(32.0, 32.0))
                .corner_radius(16.0),
        );
    }
}

fn render_theme_button(
    ui: &mut Ui,
    theme: &ThemeManager,
    config_theme: &mut SelectableTheme,
    select_theme: SelectableTheme,
    label: &str,
    icon_source: egui::ImageSource<'static>,
) {
    let current = *config_theme;
    let is_selected = current == select_theme;
    let fill_color = if is_selected {
        theme.primary
    } else {
        theme.background_elevated
    };

    let corner_radius = match select_theme {
        SelectableTheme::Neuro => CornerRadius {
            nw: 5,
            sw: 5,
            ..Default::default()
        },
        SelectableTheme::Evil => CornerRadius {
            ne: 5,
            se: 5,
            ..Default::default()
        },
        _ => CornerRadius {
            nw: 0,
            ne: 0,
            sw: 0,
            se: 0,
        },
    };

    let resp = ui
        .scope(|ui| {
            let btn_frame = egui::Frame::new()
                .fill(fill_color)
                .corner_radius(corner_radius);
            btn_frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.add(Image::new(icon_source).fit_to_exact_size(Vec2::new(20.0, 20.0)));
                    ui.add_space(6.0);
                    ui.label(RichText::new(label).size(14.0).color(theme.text));
                });
            });
        })
        .response
        .interact(Sense::click());

    if resp.hovered() {
        ui.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *config_theme = select_theme;
    }
}

fn render_twins_button(
    ui: &mut Ui,
    theme: &ThemeManager,
    config_theme: &mut SelectableTheme,
    select_theme: SelectableTheme,
    label: &str,
) {
    let current = *config_theme;
    let is_selected = current == select_theme;
    let fill_color = if is_selected {
        theme.primary
    } else {
        theme.background_elevated
    };

    let resp = ui
        .scope(|ui| {
            let btn_frame = egui::Frame::new()
                .fill(fill_color)
                .corner_radius(CornerRadius::ZERO);
            btn_frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.add(
                        Image::new(include_image!("../../assets/neuro_icon.webp"))
                            .fit_to_exact_size(Vec2::new(16.0, 16.0)),
                    );
                    ui.add_space(2.0);
                    ui.label(RichText::new(label).size(13.0).color(theme.text));
                    ui.add_space(2.0);
                    ui.add(
                        Image::new(include_image!("../../assets/evil_icon.webp"))
                            .fit_to_exact_size(Vec2::new(16.0, 16.0)),
                    );
                    ui.add_space(4.0);
                });
            });
        })
        .response
        .interact(Sense::click());

    if resp.hovered() {
        ui.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *config_theme = select_theme;
    }
}

fn render_compact_theme_btn(
    ui: &mut Ui,
    theme: &ThemeManager,
    config_theme: &mut SelectableTheme,
    select_theme: SelectableTheme,
    icon_source: egui::ImageSource<'static>,
    tooltip: &str,
) {
    let current = *config_theme;
    let is_selected = current == select_theme;
    let fill_color = if is_selected {
        theme.primary
    } else {
        theme.background_elevated
    };

    let resp = ui
        .scope(|ui| {
            let btn_frame = egui::Frame::new().fill(fill_color).corner_radius(4.0);
            btn_frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let available = ui.available_width();
                    let icon_size = 24.0;
                    let padding = (available - icon_size) / 2.0;
                    if padding > 0.0 {
                        ui.add_space(padding);
                    }
                    let mut img = Image::new(icon_source).fit_to_exact_size(Vec2::new(24.0, 24.0));
                    if is_selected {
                        img = img.tint(theme.accent_light);
                    }
                    ui.add(img);
                });
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_text(tooltip);

    if resp.hovered() {
        ui.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *config_theme = select_theme;
    }
}

fn render_compact_twins_btn(
    ui: &mut Ui,
    theme: &ThemeManager,
    config_theme: &mut SelectableTheme,
    select_theme: SelectableTheme,
    tooltip: &str,
) {
    let current = *config_theme;
    let is_selected = current == select_theme;
    let fill_color = if is_selected {
        theme.primary
    } else {
        theme.background_elevated
    };

    let resp = ui
        .scope(|ui| {
            let btn_frame = egui::Frame::new().fill(fill_color).corner_radius(4.0);
            btn_frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let available = ui.available_width();
                    let total_width = 34.0;
                    let padding = (available - total_width) / 2.0;
                    if padding > 0.0 {
                        ui.add_space(padding);
                    }
                    ui.add(
                        Image::new(include_image!("../../assets/neuro_icon.webp"))
                            .fit_to_exact_size(Vec2::new(16.0, 16.0)),
                    );
                    ui.add_space(2.0);
                    ui.add(
                        Image::new(include_image!("../../assets/evil_icon.webp"))
                            .fit_to_exact_size(Vec2::new(16.0, 16.0)),
                    );
                });
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_text(tooltip);

    if resp.hovered() {
        ui.set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *config_theme = select_theme;
    }
}

pub fn render_sidebar(
    ui: &mut Ui,
    config: &mut Config,
    theme: &ThemeManager,
    current_activity: &mut ActivityType,
    _show_queue: &mut bool,
    profile_activity: &ProfileActivity,
    config_theme: &mut SelectableTheme,
) {
    let sidebar_width = if config.compact_sidebar { 72.0 } else { 260.0 };
    Panel::left("sidebar")
        .resizable(false)
        .exact_size(sidebar_width)
        .show(ui, |ui| {
            if config.compact_sidebar {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.add(
                        Image::new(include_image!("../../assets/icon.png"))
                            .fit_to_exact_size(Vec2::new(32.0, 32.0))
                            .texture_options(TextureOptions::LINEAR),
                    );
                    ui.add_space(4.0);
                    let expand_btn = ui
                        .add(
                            Button::image(
                                Image::new(include_image!("../../assets/expand.svg"))
                                    .fit_to_exact_size(Vec2::new(20.0, 20.0)),
                            )
                            .fill(theme.background_elevated),
                        )
                        .on_hover_text("Expand Sidebar");
                    if expand_btn.clicked() {
                        config.compact_sidebar = false;
                        let _ = config.write_config();
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.add(
                        Image::new(include_image!("../../assets/icon.png"))
                            .fit_to_exact_size(Vec2::new(32.0, 32.0))
                            .texture_options(TextureOptions::LINEAR),
                    );
                    ui.with_layout(Layout::top_down(Align::Center), |ui| {
                        ui.label(
                            RichText::new(config.theme.karaoke_str())
                                .color(theme.primary_dark)
                                .size(24.0),
                        )
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let collapse_btn = ui
                            .add(
                                Button::image(
                                    Image::new(include_image!("../../assets/collapse.svg"))
                                        .fit_to_exact_size(Vec2::new(20.0, 20.0)),
                                )
                                .fill(theme.background_elevated),
                            )
                            .on_hover_text("Compact Sidebar");
                        if collapse_btn.clicked() {
                            config.compact_sidebar = true;
                            let _ = config.write_config();
                        }
                    });
                });
            }

            ui.separator();
            ui.add_space(10.0);

            let mut nav_button = |ui: &mut Ui, activity: ActivityType| {
                let resp = if config.compact_sidebar {
                    ui.scope(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let available = ui.available_width();
                            let icon_size = 24.0;
                            let padding = (available - icon_size) / 2.0;
                            if padding > 0.0 {
                                ui.add_space(padding);
                            }
                            let mut img = Image::new(activity.icon().unwrap())
                                .fit_to_exact_size(Vec2::new(24.0, 24.0));
                            if *current_activity == activity {
                                img = img.tint(theme.primary);
                            }
                            ui.add(img);
                        });
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_text(activity.as_str())
                } else {
                    ui.scope(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let mut img = Image::new(activity.icon().unwrap())
                                .fit_to_exact_size(Vec2::new(24.0, 24.0));
                            if *current_activity == activity {
                                img = img.tint(theme.primary);
                            }
                            ui.add(img);
                            ui.add_space(4.0);
                            let mut text = RichText::new(activity.as_str()).size(16.0);
                            if *current_activity == activity {
                                text = text.color(theme.primary);
                            }
                            ui.add(Label::new(text).selectable(false));
                        });
                    })
                    .response
                    .interact(Sense::click())
                };

                ui.add_space(4.0);
                if resp.hovered() {
                    ui.set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    *current_activity = activity;
                    *_show_queue = false;
                }
            };

            // nav buttons
            nav_button(ui, ActivityType::Home);
            nav_button(ui, ActivityType::Search);
            nav_button(ui, ActivityType::Radio);
            nav_button(ui, ActivityType::Playlists);
            if config.auth.is_some() {
                nav_button(ui, ActivityType::MyPlaylists);
                nav_button(ui, ActivityType::Favorites);
            }
            nav_button(ui, ActivityType::Setlists);

            // Push bottom area to the very bottom of the sidebar panel
            let bottom_height = if config.compact_sidebar { 152.0 } else { 90.0 };
            let spacing = ui.available_height() - bottom_height;
            if spacing > 0.0 {
                ui.add_space(spacing);
            }

            ui.separator();
            ui.add_space(10.0);

            // Profile Icon
            let profile_tooltip = if let Some(auth) = &config.auth {
                format!("Profile: {}", auth.user.username)
            } else {
                "Profile: Guest Account".to_string()
            };

            ui.horizontal(|ui| {
                let profile_resp = ui
                    .scope(|ui| {
                        if config.compact_sidebar {
                            ui.horizontal(|ui| {
                                let available = ui.available_width();
                                let size = 32.0;
                                let padding = (available - size) / 2.0;
                                if padding > 0.0 {
                                    ui.add_space(padding);
                                }
                                render_avatar(ui, profile_activity, theme);
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.add_space(4.0);
                                render_avatar(ui, profile_activity, theme);
                                ui.add_space(4.0);
                                if let Some(auth) = &config.auth {
                                    let username_str = &auth.user.username;
                                    ui.add(
                                        Label::new(
                                            RichText::new(username_str.to_string()).size(16.0),
                                        )
                                        .selectable(false),
                                    );
                                } else {
                                    ui.add(
                                        Label::new(
                                            RichText::new("Guest Account")
                                                .italics()
                                                .color(theme.text_muted)
                                                .size(14.0),
                                        )
                                        .selectable(false),
                                    );
                                }
                            });
                        }
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_text(profile_tooltip);

                if profile_resp.hovered() {
                    ui.set_cursor_icon(CursorIcon::PointingHand);
                }

                if profile_resp.clicked() {
                    *current_activity = ActivityType::Profile;
                }

                if !config.compact_sidebar {
                    // Settings icon on the right side of the profile name
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(4.0);
                        let mut settings_img = Image::new(ActivityType::Settings.icon().unwrap())
                            .fit_to_exact_size(Vec2::new(20.0, 20.0));
                        if *current_activity == ActivityType::Settings {
                            settings_img = settings_img.tint(theme.primary);
                        }
                        let settings_resp = ui
                            .add(settings_img)
                            .interact(Sense::click())
                            .on_hover_text("Settings");
                        if settings_resp.hovered() {
                            ui.set_cursor_icon(CursorIcon::PointingHand);
                        }
                        if settings_resp.clicked() {
                            *current_activity = ActivityType::Settings;
                            *_show_queue = false;
                        }
                    });
                }
            });

            ui.add_space(10.0);

            // theme switcher
            if config.compact_sidebar {
                ui.vertical(|ui| {
                    render_compact_theme_btn(
                        ui,
                        theme,
                        config_theme,
                        SelectableTheme::Neuro,
                        include_image!("../../assets/neuro_icon.webp"),
                        "Neuro Theme",
                    );
                    ui.add_space(4.0);
                    render_compact_twins_btn(
                        ui,
                        theme,
                        config_theme,
                        SelectableTheme::Twins,
                        "Twins Theme",
                    );
                    ui.add_space(4.0);
                    render_compact_theme_btn(
                        ui,
                        theme,
                        config_theme,
                        SelectableTheme::Evil,
                        include_image!("../../assets/evil_icon.webp"),
                        "Evil Theme",
                    );
                });
            } else {
                ui.horizontal(|ui| {
                    let spacing = ui.spacing_mut();
                    let old_spacing = (spacing.item_spacing, spacing.button_padding);
                    spacing.item_spacing = Vec2::default();
                    spacing.button_padding = Vec2::default();

                    ui.columns(3, |columns| {
                        render_theme_button(
                            &mut columns[0],
                            theme,
                            config_theme,
                            SelectableTheme::Neuro,
                            "Neuro",
                            include_image!("../../assets/neuro_icon.webp"),
                        );
                        render_twins_button(
                            &mut columns[1],
                            theme,
                            config_theme,
                            SelectableTheme::Twins,
                            "Twins",
                        );
                        render_theme_button(
                            &mut columns[2],
                            theme,
                            config_theme,
                            SelectableTheme::Evil,
                            "Evil",
                            include_image!("../../assets/evil_icon.webp"),
                        );
                    });

                    let spacing = ui.spacing_mut();
                    (spacing.item_spacing, spacing.button_padding) = old_spacing;
                });
            }

            if config.compact_sidebar {
                ui.add_space(10.0);
            }
        });
}
