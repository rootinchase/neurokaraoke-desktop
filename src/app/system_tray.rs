use crate::theme::SelectableTheme;
use eframe::egui;
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

fn load_icon() -> Icon {
    let image = image::load_from_memory(include_bytes!("../../assets/icon.png"))
        .expect("Failed to load tray icon from memory")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    Icon::from_rgba(rgba, width, height).unwrap()
}

pub struct TrayIconMenu {
    _tray_icon: TrayIcon,
    _tray_menu: Menu,
    _show_item: MenuItem,
    play_pause_item: MenuItem,
    _theme_submenu: Submenu,
    theme_neuro: MenuItem,
    theme_twins: MenuItem,
    theme_evil: MenuItem,
    _timer_submenu: Submenu,
    _timer_5: MenuItem,
    _timer_15: MenuItem,
    _timer_30: MenuItem,
    _timer_60: MenuItem,
    _timer_120: MenuItem,
    timer_cancel: MenuItem,
    _quit_item: MenuItem,
    _show_id: MenuId,
    play_pause_id: MenuId,
    theme_neuro_id: MenuId,
    theme_twins_id: MenuId,
    theme_evil_id: MenuId,
    timer_5_id: MenuId,
    timer_15_id: MenuId,
    timer_30_id: MenuId,
    timer_60_id: MenuId,
    timer_120_id: MenuId,
    timer_cancel_id: MenuId,
    quit_id: MenuId,
    _is_visible: bool,
    last_is_paused: Option<bool>,
    last_theme: Option<SelectableTheme>,
}

impl TrayIconMenu {
    pub(crate) fn new(_ctx: &egui::Context) -> Self {
        let _tray_menu = Menu::new();
        let _show_item = MenuItem::new("Hide App", true, None);
        let play_pause_item = MenuItem::new("Play", true, None);

        let _theme_submenu = Submenu::new("Theme", true);
        let theme_neuro = MenuItem::new("Neuro", true, None);
        let theme_twins = MenuItem::new("Twins", true, None);
        let theme_evil = MenuItem::new("Evil", true, None);
        let _ = _theme_submenu.append_items(&[&theme_neuro, &theme_twins, &theme_evil]);

        let _timer_submenu = Submenu::new("Sleep Timer", true);
        let _timer_5 = MenuItem::new("5 min", true, None);
        let _timer_15 = MenuItem::new("15 min", true, None);
        let _timer_30 = MenuItem::new("30 min", true, None);
        let _timer_60 = MenuItem::new("60 min", true, None);
        let _timer_120 = MenuItem::new("120 min", true, None);
        let timer_cancel = MenuItem::new("Cancel Timer", true, None);
        let _ = _timer_submenu.append_items(&[
            &_timer_5,
            &_timer_15,
            &_timer_30,
            &_timer_60,
            &_timer_120,
            &timer_cancel,
        ]);

        let _quit_item = MenuItem::new("Quit", true, None);

        let _ = _tray_menu.append_items(&[
            &_show_item,
            &play_pause_item,
            &_theme_submenu,
            &_timer_submenu,
            &_quit_item,
        ]);

        let _show_id = _show_item.id().clone();
        let play_pause_id = play_pause_item.id().clone();
        let theme_neuro_id = theme_neuro.id().clone();
        let theme_twins_id = theme_twins.id().clone();
        let theme_evil_id = theme_evil.id().clone();
        let timer_5_id = _timer_5.id().clone();
        let timer_15_id = _timer_15.id().clone();
        let timer_30_id = _timer_30.id().clone();
        let timer_60_id = _timer_60.id().clone();
        let timer_120_id = _timer_120.id().clone();
        let timer_cancel_id = timer_cancel.id().clone();
        let quit_id = _quit_item.id().clone();

        // 2. Load icon from embedded bytes
        let icon = load_icon();

        // 3. Build the tray icon
        let _tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(_tray_menu.clone()))
            .with_tooltip("Neuro Karaoke")
            .with_icon(icon)
            .build()
            .unwrap();

        Self {
            _tray_icon,
            _tray_menu,
            _show_item,
            play_pause_item,
            _theme_submenu,
            theme_neuro,
            theme_twins,
            theme_evil,
            _timer_submenu,
            _timer_5,
            _timer_15,
            _timer_30,
            _timer_60,
            _timer_120,
            timer_cancel,
            _quit_item,
            _show_id,
            play_pause_id,
            theme_neuro_id,
            theme_twins_id,
            theme_evil_id,
            timer_5_id,
            timer_15_id,
            timer_30_id,
            timer_60_id,
            timer_120_id,
            timer_cancel_id,
            quit_id,
            _is_visible: true,
            last_is_paused: None,
            last_theme: None,
        }
    }

    pub fn poll_events(
        &mut self,
        ctx: &egui::Context,
        player: &crate::audio::Player,
        config: &mut crate::config::Config,
        theme: &mut crate::theme::ThemeManager,
        sleep_timer_end: &mut Option<Instant>,
    ) {
        let is_paused = player.get_playback_state().map_or(true, |s| s.paused());
        if self.last_is_paused != Some(is_paused) {
            self.last_is_paused = Some(is_paused);
            if is_paused {
                self.play_pause_item.set_text("Play");
            } else {
                self.play_pause_item.set_text("Pause");
            }
        }

        self.timer_cancel.set_enabled(sleep_timer_end.is_some());

        let current_theme = config.theme;
        if self.last_theme != Some(current_theme) {
            self.last_theme = Some(current_theme);
            self.theme_neuro
                .set_text(if current_theme == SelectableTheme::Neuro {
                    "✓ Neuro"
                } else {
                    "Neuro"
                });
            self.theme_twins
                .set_text(if current_theme == SelectableTheme::Twins {
                    "✓ Twins"
                } else {
                    "Twins"
                });
            self.theme_evil
                .set_text(if current_theme == SelectableTheme::Evil {
                    "✓ Evil"
                } else {
                    "Evil"
                });
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.play_pause_id {
                if let Some(state) = player.get_playback_state() {
                    if state.paused() {
                        player.play();
                    } else {
                        player.pause();
                    }
                } else {
                    player.play();
                }
                ctx.request_repaint();
            } else if event.id == self.theme_neuro_id {
                config.theme = SelectableTheme::Neuro;
                theme.set(config.theme.as_theme());
                let _ = config.write_config();
                ctx.request_repaint();
            } else if event.id == self.theme_twins_id {
                config.theme = SelectableTheme::Twins;
                theme.set(config.theme.as_theme());
                let _ = config.write_config();
                ctx.request_repaint();
            } else if event.id == self.theme_evil_id {
                config.theme = SelectableTheme::Evil;
                theme.set(config.theme.as_theme());
                let _ = config.write_config();
                ctx.request_repaint();
            } else if event.id == self.timer_5_id {
                *sleep_timer_end = Some(Instant::now() + Duration::from_secs(5 * 60));
                ctx.request_repaint();
            } else if event.id == self.timer_15_id {
                *sleep_timer_end = Some(Instant::now() + Duration::from_secs(15 * 60));
                ctx.request_repaint();
            } else if event.id == self.timer_30_id {
                *sleep_timer_end = Some(Instant::now() + Duration::from_secs(30 * 60));
                ctx.request_repaint();
            } else if event.id == self.timer_60_id {
                *sleep_timer_end = Some(Instant::now() + Duration::from_secs(60 * 60));
                ctx.request_repaint();
            } else if event.id == self.timer_120_id {
                *sleep_timer_end = Some(Instant::now() + Duration::from_secs(120 * 60));
                ctx.request_repaint();
            } else if event.id == self.timer_cancel_id {
                *sleep_timer_end = None;
                ctx.request_repaint();
            } else if event.id == self.quit_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                std::process::exit(0);
            }
        }
    }
}
