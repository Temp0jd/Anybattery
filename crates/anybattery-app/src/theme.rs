use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Vec2,
};
use std::sync::Arc;

use anybattery_core::Theme;

/// 8BitDo-inspired palette: warm plastic, ink outlines, red buttons.
pub const ACCENT: Color32 = Color32::from_rgb(200, 68, 60);
pub const GREEN: Color32 = Color32::from_rgb(79, 138, 91);
pub const ORANGE: Color32 = Color32::from_rgb(217, 138, 43);
pub const RED: Color32 = Color32::from_rgb(142, 47, 41);
pub const YELLOW: Color32 = Color32::from_rgb(230, 190, 80);

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub bg_glow: Color32,
    pub card: Color32,
    pub card_solid: Color32,
    pub card_border: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub tertiary: Color32,
    pub track: Color32,
    pub control: Color32,
    pub hover: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub red: Color32,
    pub shadow: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            bg: Color32::from_rgb(35, 33, 29),
            bg_glow: Color32::from_rgb(45, 42, 36),
            card: Color32::from_rgb(50, 47, 41),
            card_solid: Color32::from_rgb(58, 54, 47),
            card_border: Color32::from_rgb(232, 226, 212),
            text: Color32::from_rgb(243, 238, 223),
            secondary: Color32::from_rgb(184, 176, 158),
            tertiary: Color32::from_rgb(132, 125, 110),
            track: Color32::from_rgb(74, 69, 60),
            control: Color32::from_rgb(64, 60, 52),
            hover: Color32::from_rgb(78, 73, 64),
            accent: Color32::from_rgb(224, 90, 80),
            green: Color32::from_rgb(112, 168, 118),
            orange: Color32::from_rgb(230, 160, 70),
            red: Color32::from_rgb(224, 90, 80),
            shadow: Color32::from_rgb(12, 11, 9),
        }
    } else {
        Palette {
            dark,
            bg: Color32::from_rgb(232, 226, 212),
            bg_glow: Color32::from_rgb(244, 239, 228),
            card: Color32::from_rgb(247, 242, 231),
            card_solid: Color32::from_rgb(251, 247, 238),
            card_border: Color32::from_rgb(46, 43, 38),
            text: Color32::from_rgb(43, 40, 35),
            secondary: Color32::from_rgb(111, 106, 94),
            tertiary: Color32::from_rgb(154, 148, 134),
            track: Color32::from_rgb(214, 207, 192),
            control: Color32::from_rgb(227, 220, 205),
            hover: Color32::from_rgb(239, 231, 216),
            accent: ACCENT,
            green: GREEN,
            orange: ORANGE,
            red: RED,
            shadow: Color32::from_rgb(46, 43, 38),
        }
    }
}

pub fn resolve_dark(theme: Theme) -> bool {
    match theme {
        Theme::System => anybattery_windows::system::is_dark_mode(),
        Theme::Light => false,
        Theme::Dark => true,
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "pixel".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/FusionPixel12-zh_hans.otf"
        ))),
    );

    // The pixel font covers GB2312; a system CJK face is only a fallback for rare glyphs.
    let mut fallback: Option<String> = None;
    for path in [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "system-cjk".to_owned(),
                Arc::new(FontData::from_owned(bytes)),
            );
            fallback = Some("system-cjk".to_owned());
            break;
        }
    }

    let mut regular = vec!["pixel".to_owned()];
    let mut bold = vec!["pixel".to_owned()];
    if let Some(fallback) = &fallback {
        regular.push(fallback.clone());
        bold.push(fallback.clone());
    }
    fonts.families.insert(FontFamily::Proportional, regular.clone());
    fonts.families.insert(FontFamily::Monospace, regular);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(24.0, FontFamily::Name("bold".into()))),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(6.0, 6.0);
        style.spacing.button_padding = Vec2::new(8.0, 5.0);
        style.spacing.interact_size = Vec2::new(32.0, 28.0);
    });
}

pub fn apply_visuals(ctx: &egui::Context, p: Palette, animations: bool) {
    let mut visuals = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.override_text_color = Some(p.text);
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_fill = p.card_solid;
    visuals.faint_bg_color = p.control;
    visuals.extreme_bg_color = p.track;
    visuals.selection.bg_fill = p.accent;
    visuals.selection.stroke = Stroke::new(2.0, p.text);
    visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.weak_bg_fill = p.control;
    visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, p.card_border);
    visuals.widgets.inactive.corner_radius = CornerRadius::ZERO;
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, p.card_border);
    visuals.widgets.hovered.corner_radius = CornerRadius::ZERO;
    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, p.card_border);
    visuals.widgets.active.corner_radius = CornerRadius::ZERO;
    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
    let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(if p.dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    ctx.set_visuals_of(theme, visuals);
    ctx.style_mut_of(theme, |style| {
        style.animation_time = if animations { 0.08 } else { 0.0 };
    });
}

pub fn rich(value: impl Into<String>, size: f32, color: Color32, bold: bool) -> egui::RichText {
    let family = if bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    egui::RichText::new(value.into()).size(size).color(color).family(family)
}

pub fn battery_color(p: Palette, level: u8, charging: bool) -> Color32 {
    if charging {
        p.green
    } else if level <= 10 {
        p.red
    } else if level <= 20 {
        p.orange
    } else {
        p.accent
    }
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}
