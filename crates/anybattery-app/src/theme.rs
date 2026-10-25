use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Vec2,
};
use std::sync::Arc;

use anybattery_core::Theme;

pub const ACCENT: Color32 = Color32::from_rgb(0, 122, 255);
pub const GREEN: Color32 = Color32::from_rgb(52, 199, 89);
pub const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
pub const RED: Color32 = Color32::from_rgb(255, 59, 48);
pub const YELLOW: Color32 = Color32::from_rgb(255, 204, 0);

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
            bg: Color32::from_rgb(28, 28, 30),
            bg_glow: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
            card: Color32::from_rgba_unmultiplied(58, 58, 60, 212),
            card_solid: Color32::from_rgb(44, 44, 46),
            card_border: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            text: Color32::from_rgb(245, 245, 247),
            secondary: Color32::from_rgb(161, 161, 166),
            tertiary: Color32::from_rgb(110, 110, 115),
            track: Color32::from_rgba_unmultiplied(118, 118, 128, 48),
            control: Color32::from_rgba_unmultiplied(118, 118, 128, 40),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
            accent: Color32::from_rgb(10, 132, 255),
            green: Color32::from_rgb(48, 209, 88),
            orange: Color32::from_rgb(255, 159, 10),
            red: Color32::from_rgb(255, 69, 58),
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 90),
        }
    } else {
        Palette {
            dark,
            bg: Color32::from_rgb(242, 242, 247),
            bg_glow: Color32::from_rgba_unmultiplied(255, 255, 255, 210),
            card: Color32::from_rgba_unmultiplied(255, 255, 255, 224),
            card_solid: Color32::from_rgb(255, 255, 255),
            card_border: Color32::from_rgba_unmultiplied(0, 0, 0, 14),
            text: Color32::from_rgb(29, 29, 31),
            secondary: Color32::from_rgb(110, 110, 115),
            tertiary: Color32::from_rgb(161, 161, 166),
            track: Color32::from_rgba_unmultiplied(118, 118, 128, 28),
            control: Color32::from_rgba_unmultiplied(118, 118, 128, 20),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            accent: ACCENT,
            green: GREEN,
            orange: ORANGE,
            red: RED,
            shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
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
        "anybattery-regular".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/NotoSansSC-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        "anybattery-bold".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/NotoSansSC-Bold.otf"
        ))),
    );
    fonts.families.insert(
        FontFamily::Name("bold".into()),
        vec!["anybattery-bold".to_owned(), "anybattery-regular".to_owned()],
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "anybattery-regular".to_owned());
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(26.0, FontFamily::Name("bold".into()))),
            (TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(10.0, 6.0);
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
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.weak_bg_fill = p.control;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.active.bg_fill = p.hover;
    visuals.widgets.active.weak_bg_fill = p.hover;
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.window_corner_radius = 20.into();
    visuals.menu_corner_radius = 12.into();
    let theme = if p.dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_theme(if p.dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
    ctx.set_visuals_of(theme, visuals);
    ctx.style_mut_of(theme, |style| {
        style.animation_time = if animations { 0.16 } else { 0.0 };
    });
}

pub fn text(size: f32, color: Color32, bold: bool) -> egui::RichText {
    let family = if bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    egui::RichText::new("").size(size).color(color).family(family)
}

pub fn rich(value: impl Into<String>, size: f32, color: Color32, bold: bool) -> egui::RichText {
    let family = if bold {
        FontFamily::Name("bold".into())
    } else {
        FontFamily::Proportional
    };
    egui::RichText::new(value.into())
        .size(size)
        .color(color)
        .family(family)
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
