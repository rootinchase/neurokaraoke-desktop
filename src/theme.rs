use eframe::egui;
use eframe::egui::{Color32, Rgba, lerp};
use serde::{Deserialize, Serialize};
use std::ops::Deref;

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
        let lerp_color = |a: Color32, b: Color32| lerp(Rgba::from(a)..=Rgba::from(b), t).into();

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
            Color32::from_hex("#00D9FF").unwrap(), // primary
            Color32::from_hex("#5CE1FF").unwrap(), // primary_light
            Color32::from_hex("#00B8D4").unwrap(), // primary_dark
            Color32::from_hex("#FF6B9D").unwrap(), // accent
            Color32::from_hex("#FFB3D1").unwrap(), // accent_light
            Color32::from_hex("#0A0E1A").unwrap(), // background
            Color32::from_hex("#141B2D").unwrap(), // background_elevated
            Color32::from_hex("#1E2A42").unwrap(), // background_mid
            Color32::from_hex("#2A3A58").unwrap(), // background_hover
            Color32::WHITE,                        // text
            Color32::from_hex("#A8B9D9").unwrap(), // text_secondary
            Color32::from_hex("#6B7A98").unwrap(), // text_muted
            Color32::from_hex("#38465E").unwrap(), // border
            Color32::from_hex("#00D9FF").unwrap(), // border_focus
            Color32::from_hex("#22C55E").unwrap(), // success
            Color32::from_hex("#F59E0B").unwrap(), // warning
            Color32::from_hex("#EF4444").unwrap(), // error
        )
    }

    pub fn evil() -> Self {
        Self::new(
            Color32::from_hex("#FF0066").unwrap(), // primary
            Color32::from_hex("#FF3385").unwrap(), // primary_ligh
            Color32::from_hex("#CC0052").unwrap(), // primary_dark
            Color32::from_hex("#9D00FF").unwrap(), // accent
            Color32::from_hex("#B84DFF").unwrap(), // accent_light
            Color32::from_hex("#0D0208").unwrap(), // background
            Color32::from_hex("#1A0B14").unwrap(), // background_elevated
            Color32::from_hex("#2A1220").unwrap(), // background_mid
            Color32::from_hex("#3D1A2E").unwrap(), // background_hover
            Color32::WHITE,                              // text
            Color32::from_hex("#E0A3C7").unwrap(), // text_secondary
            Color32::from_hex("#8A5A73").unwrap(), // text_muted
            Color32::from_hex("#4A2438").unwrap(), // border
            Color32::from_hex("#FF0066").unwrap(), // border_focu
            Color32::from_hex("#22C55E").unwrap(), // success
            Color32::from_hex("#F59E0B").unwrap(), // warning
            Color32::from_hex("#EF4444").unwrap(), // erro
        )
    }

    pub fn twins() -> Self {
        Self::new(
            Color32::from_hex("#9D5CFF").unwrap(), // primary
            Color32::from_hex("#B98AFF").unwrap(), // primary_ligh
            Color32::from_hex("#7A3FD9").unwrap(), // primary_dark
            Color32::from_hex("#FF6B9D").unwrap(), // accent
            Color32::from_hex("#5CE1FF").unwrap(), // accent_light
            Color32::from_hex("#0A0814").unwrap(), // background
            Color32::from_hex("#150F23").unwrap(), // background_elevated
            Color32::from_hex("#221A35").unwrap(), // background_mid
            Color32::from_hex("#2F2345").unwrap(), // background_hover
            Color32::WHITE,                        // text
            Color32::from_hex("#C5B3E0").unwrap(), // text_secondary
            Color32::from_hex("#7A6B98").unwrap(), // text_muted
            Color32::from_hex("#40345A").unwrap(), // border
            Color32::from_hex("#9D5CFF").unwrap(), // border_focu
            Color32::from_hex("#22C55E").unwrap(), // success
            Color32::from_hex("#F59E0B").unwrap(), // warning
            Color32::from_hex("#EF4444").unwrap(), // erro
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
            self.current = Theme::lerp(
                &self.from,
                &self.to,
                0.5 * (1.0 - (std::f32::consts::PI * self.t).cos()),
            ); // sine ease in/out
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SelectableTheme {
    Neuro,
    Evil,
    Twins,
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

impl Default for SelectableTheme {
    fn default() -> Self {
        Self::Neuro
    }
}
