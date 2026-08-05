use std::ops::{Deref, DerefMut};
use eframe::egui::{lerp, Color32, Rgba};
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    // Brand
    pub primary: Color32,
    pub primary_light: Color32,
    pub primary_dark: Color32,

    pub accent: Color32,
    pub accent_light: Color32,

    // Backgrounds
    pub background: Color32,
    pub background_secondary: Color32,
    pub background_mid: Color32,
    pub background_elevated: Color32,
    pub background_hover: Color32,

    // Text
    pub text: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,

    // UI
    pub border: Color32,
    pub border_focus: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
}

impl Theme {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        primary: Color32,
        primary_light: Color32,
        primary_dark: Color32,
        accent: Color32,
        accent_light: Color32,
        background: Color32,
        background_secondary: Color32,
        background_elevated: Color32,
        background_hover: Color32,
        text: Color32,
        text_secondary: Color32,
        text_muted: Color32,
        border: Color32,
        border_focus: Color32,
        success: Color32,
        warning: Color32,
        error: Color32,
    ) -> Self {
        Self {
            primary,
            primary_light,
            primary_dark,

            accent,
            accent_light,

            background,
            background_secondary,
            background_mid: lerp(
                Rgba::from(background)..=Rgba::from(background_secondary),
                0.5,
            )
                .into(),
            background_elevated,
            background_hover,

            text,
            text_secondary,
            text_muted,

            border,
            border_focus,
            success,
            warning,
            error,
        }
    }

    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let lerp_color = |a: Color32, b: Color32| {
            lerp(Rgba::from(a)..=Rgba::from(b), t).into()
        };

        Self {
            primary: lerp_color(a.primary, b.primary),
            primary_light: lerp_color(a.primary_light, b.primary_light),
            primary_dark: lerp_color(a.primary_dark, b.primary_dark),

            accent: lerp_color(a.accent, b.accent),
            accent_light: lerp_color(a.accent_light, b.accent_light),

            background: lerp_color(a.background, b.background),
            background_secondary: lerp_color(a.background_secondary, b.background_secondary),
            background_mid: lerp_color(a.background_mid, b.background_mid),
            background_elevated: lerp_color(a.background_elevated, b.background_elevated),
            background_hover: lerp_color(a.background_hover, b.background_hover),

            text: lerp_color(a.text, b.text),
            text_secondary: lerp_color(a.text_secondary, b.text_secondary),
            text_muted: lerp_color(a.text_muted, b.text_muted),

            border: lerp_color(a.border, b.border),
            border_focus: lerp_color(a.border_focus, b.border_focus),

            success: lerp_color(a.success, b.success),
            warning: lerp_color(a.warning, b.warning),
            error: lerp_color(a.error, b.error),
        }
    }

    pub fn visuals(&self) -> egui::Visuals {
        let mut v = egui::Visuals::dark();

        v.override_text_color = Some(self.text);

        v.window_fill = self.background;
        v.panel_fill = self.background_secondary;
        v.faint_bg_color = self.background_mid;
        v.extreme_bg_color = self.background_elevated;

        v.hyperlink_color = self.primary;

        v.selection.bg_fill = self.primary;
        v.selection.stroke.color = self.text;

        v.widgets.noninteractive.bg_fill = self.background;
        v.widgets.inactive.bg_fill = self.background_elevated;
        v.widgets.hovered.bg_fill = self.background_hover;
        v.widgets.active.bg_fill = self.primary;

        v.widgets.noninteractive.weak_bg_fill = self.background;
        v.widgets.inactive.weak_bg_fill = self.background_elevated;
        v.widgets.hovered.weak_bg_fill = self.background_hover;
        v.widgets.active.weak_bg_fill = self.primary;

        v.widgets.inactive.fg_stroke.color = self.text;
        v.widgets.hovered.fg_stroke.color = self.text;
        v.widgets.active.fg_stroke.color = self.text;

        v.widgets.inactive.bg_stroke.color = self.border;
        v.widgets.hovered.bg_stroke.color = self.border_focus;
        v.widgets.active.bg_stroke.color = self.border_focus;

        v
    }

    pub fn neuro() -> Self {
        Self::new(
            Color32::from_rgb(0x00, 0xD9, 0xFF),
            Color32::from_rgb(0x5C, 0xE1, 0xFF),
            Color32::from_rgb(0x00, 0xB8, 0xD4),
            Color32::from_rgb(0xFF, 0x6B, 0x9D),
            Color32::from_rgb(0xFF, 0xB3, 0xD1),
            Color32::from_rgb(0x0A, 0x0E, 0x1A),
            Color32::from_rgb(0x14, 0x1B, 0x2D),
            Color32::from_rgb(0x1E, 0x2A, 0x42),
            Color32::from_rgb(0x2A, 0x3A, 0x58),
            Color32::WHITE,
            Color32::from_rgb(0xA8, 0xB9, 0xD9),
            Color32::from_rgb(0x6B, 0x7A, 0x98),
            Color32::from_rgb(0x38, 0x46, 0x5E),
            Color32::from_rgb(0x00, 0xD9, 0xFF),
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            Color32::from_rgb(0xF5, 0x9E, 0x0B),
            Color32::from_rgb(0xEF, 0x44, 0x44),
        )
    }

    pub fn evil() -> Self {
        Self::new(
            Color32::from_rgb(0xFF, 0x00, 0x66),
            Color32::from_rgb(0xFF, 0x33, 0x85),
            Color32::from_rgb(0xCC, 0x00, 0x52),
            Color32::from_rgb(0x9D, 0x00, 0xFF),
            Color32::from_rgb(0xB8, 0x4D, 0xFF),
            Color32::from_rgb(0x0D, 0x02, 0x08),
            Color32::from_rgb(0x1A, 0x0B, 0x14),
            Color32::from_rgb(0x2A, 0x12, 0x20),
            Color32::from_rgb(0x3D, 0x1A, 0x2E),
            Color32::WHITE,
            Color32::from_rgb(0xE0, 0xA3, 0xC7),
            Color32::from_rgb(0x8A, 0x5A, 0x73),
            Color32::from_rgb(0x4A, 0x24, 0x38),
            Color32::from_rgb(0xFF, 0x00, 0x66),
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            Color32::from_rgb(0xF5, 0x9E, 0x0B),
            Color32::from_rgb(0xEF, 0x44, 0x44),
        )
    }

    pub fn twins() -> Self {
        Self::new(
            Color32::from_rgb(0x9D, 0x5C, 0xFF),
            Color32::from_rgb(0xB9, 0x8A, 0xFF),
            Color32::from_rgb(0x7A, 0x3F, 0xD9),
            Color32::from_rgb(0xFF, 0x6B, 0x9D),
            Color32::from_rgb(0x5C, 0xE1, 0xFF),
            Color32::from_rgb(0x0A, 0x08, 0x14),
            Color32::from_rgb(0x15, 0x0F, 0x23),
            Color32::from_rgb(0x22, 0x1A, 0x35),
            Color32::from_rgb(0x2F, 0x23, 0x45),
            Color32::WHITE,
            Color32::from_rgb(0xC5, 0xB3, 0xE0),
            Color32::from_rgb(0x7A, 0x6B, 0x98),
            Color32::from_rgb(0x40, 0x34, 0x5A),
            Color32::from_rgb(0x9D, 0x5C, 0xFF),
            Color32::from_rgb(0x22, 0xC5, 0x5E),
            Color32::from_rgb(0xF5, 0x9E, 0x0B),
            Color32::from_rgb(0xEF, 0x44, 0x44),
        )
    }
}

pub struct ThemeManager {
    current: Theme,
    from: Theme,
    to: Theme,
    t: f32,
}

impl ThemeManager {
    pub fn new(theme: Theme) -> Self {
        Self {
            current: theme,
            from: theme,
            to: theme,
            t: 0.9999999,
        }
    }

    pub fn animate(&mut self, dt: f32) -> bool {
        if self.t < 1.0 {
            self.t += dt / 2.0; // 2.0s animation time
            self.current = Theme::lerp(&self.from, &self.to, 0.5 * (1.0 - (std::f32::consts::PI * self.t).cos())); // sine ease in/out
            true
        } else {
            self.current = self.to;
            false
        }
    }

    pub fn set(&mut self, theme: Theme) {
        self.from = self.current;
        self.to = theme;
        self.t = 0.0;
    }
}

impl Deref for ThemeManager {
    type Target = Theme;
    fn deref(&self) -> &Self::Target {
        &self.current
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SelectableTheme {
    Neuro, Evil, Twins
}

impl SelectableTheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Neuro => "Neuro",
            Self::Evil => "Evil",
            Self::Twins => "Twins",
        }
    }

    pub fn karaoke_str(&self) -> &'static str {
        match self {
            Self::Neuro => "Neuro Karaoke",
            Self::Evil => "Evil Karaoke",
            Self::Twins => "Twins Karaoke",
        }
    }

    pub fn as_theme(&self) -> Theme {
        match self {
            Self::Neuro => Theme::neuro(),
            Self::Evil => Theme::evil(),
            Self::Twins => Theme::twins(),
        }
    }
}