use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, FontTweak, Margin, Stroke,
    TextStyle, Theme, ThemePreference, Visuals,
};
use std::path::Path;
use std::sync::Arc;

pub const ACCENT: Color32 = Color32::from_rgb(0x1D, 0x5F, 0xD8);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x18, 0x50, 0xB8);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xEB, 0xF1, 0xFD);
pub const DISABLED_FILL: Color32 = Color32::from_rgb(0xE7, 0xE7, 0xEA);
pub const DISABLED_TEXT: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x93);
pub const CHECK_BORDER: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x90);

pub const BG: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xFA);
pub const SIDEBAR: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
pub const CARD: Color32 = Color32::WHITE;
pub const RAISED: Color32 = Color32::from_rgb(0xF4, 0xF4, 0xF5);
pub const HOVER: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xEE);
pub const BORDER: Color32 = Color32::from_rgb(0xE2, 0xE2, 0xE5);
pub const CONTROL_BORDER: Color32 = Color32::from_rgb(0xD4, 0xD4, 0xD8);
pub const TRACK: Color32 = Color32::from_rgb(0xEC, 0xEC, 0xEE);

pub const TEXT: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
pub const MUTED: Color32 = Color32::from_rgb(0x52, 0x52, 0x52);
pub const SUBTLE: Color32 = Color32::from_rgb(0x73, 0x73, 0x73);
pub const MONO_INK: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x40);
pub const SECONDARY_INK: Color32 = Color32::from_rgb(0x5E, 0x5E, 0x64);

pub const SUCCESS: Color32 = Color32::from_rgb(0x16, 0x80, 0x3C);
pub const WARN: Color32 = Color32::from_rgb(0xA1, 0x52, 0x07);
pub const WARN_SOFT: Color32 = Color32::from_rgb(0xFE, 0xF8, 0xEB);
pub const WARN_BORDER: Color32 = Color32::from_rgb(0xF3, 0xDF, 0xB5);
pub const DANGER: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
pub const DANGER_SOFT: Color32 = Color32::from_rgb(0xFE, 0xF3, 0xF2);
pub const DANGER_BORDER: Color32 = Color32::from_rgb(0xF5, 0xCF, 0xCC);

const MONO_BASELINE: f32 = 0.18;

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn mono_soft_family() -> FontFamily {
    FontFamily::Name("mono_soft".into())
}

pub fn mono_soft(size: f32) -> FontId {
    FontId::new(size, mono_soft_family())
}

fn load_font(name: &str) -> Option<Vec<u8>> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    std::fs::read(Path::new(&windir).join("Fonts").join(name)).ok()
}

pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |id: &str, files: &[&str], y_offset: f32| -> bool {
        for f in files {
            if let Some(bytes) = load_font(f) {
                let data = FontData::from_owned(bytes).tweak(FontTweak { y_offset_factor: y_offset, ..Default::default() });
                fonts.font_data.insert(id.to_string(), Arc::new(data));
                return true;
            }
        }
        false
    };
    let has_regular = add("segoe", &["segoeui.ttf"], 0.0);
    let has_semibold = add("segoe_sb", &["seguisb.ttf", "segoeuib.ttf"], 0.0);
    let has_icons = add("icons", &["SegoeIcons.ttf", "segmdl2.ttf"], 0.0);
    // Consolas sits higher than Segoe UI on a shared line; nudge it onto the same baseline.
    let has_mono = add("mono", &["consola.ttf", "CascadiaMono.ttf"], MONO_BASELINE);
    let has_mono_soft = add("mono_soft", &["consola.ttf", "CascadiaMono.ttf"], MONO_BASELINE);

    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if has_regular {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    if has_mono {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".into());
    }
    let mut soft_list = Vec::new();
    if has_mono_soft {
        soft_list.push("mono_soft".to_string());
    }
    soft_list.extend(fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default());
    fonts.families.insert(mono_soft_family(), soft_list);
    let mut semibold_list = Vec::new();
    if has_semibold {
        semibold_list.push("segoe_sb".to_string());
    }
    if has_regular {
        semibold_list.push("segoe".to_string());
    }
    semibold_list.extend(proportional.iter().cloned());
    fonts.families.insert(semibold(), semibold_list);
    let mut icon_list = Vec::new();
    if has_icons {
        icon_list.push("icons".to_string());
    }
    icon_list.extend(proportional);
    fonts.families.insert(icons(), icon_list);
    ctx.set_fonts(fonts);

    ctx.set_theme(ThemePreference::Light);
    ctx.style_mut_of(Theme::Light, |style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(28.0, FontFamily::Proportional)),
            (TextStyle::Monospace, mono(12.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.window_margin = Margin::same(24);
        style.spacing.menu_margin = Margin::same(4);
        style.spacing.icon_width = 18.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating_width = 3.0;
        style.spacing.scroll.bar_inner_margin = 0.0;
        style.spacing.scroll.bar_outer_margin = 3.0;

        let mut v = Visuals::light();
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.extreme_bg_color = CARD;
        v.faint_bg_color = RAISED;
        v.window_stroke = Stroke::new(1.0_f32, BORDER);
        v.window_corner_radius = CornerRadius::same(16);
        v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(16) };
        v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: Color32::from_black_alpha(14) };
        v.menu_corner_radius = CornerRadius::same(10);
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = ACCENT_SOFT;
        v.selection.stroke = Stroke::new(1.5_f32, ACCENT);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(6);
            w.expansion = 0.0;
        }
        v.widgets.inactive.bg_fill = CARD;
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, CONTROL_BORDER);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.hovered.bg_fill = CARD;
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0xA1, 0xA1, 0xAA));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(0xE4, 0xE4, 0xE7);
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
        v.widgets.open.weak_bg_fill = HOVER;
        v.widgets.open.bg_stroke = Stroke::new(1.0_f32, CONTROL_BORDER);
        style.visuals = v;
    });
}
