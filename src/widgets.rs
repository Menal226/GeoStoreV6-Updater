use crate::i18n::Lang;
use crate::t;
use crate::theme::{self, *};
use eframe::egui::{
    self, text::LayoutJob, Align, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Frame,
    Layout, Margin, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextFormat,
    TextWrapMode, Ui, Vec2, WidgetText,
};

pub mod icon {
    pub const HOME: char = '\u{E80F}';
    pub const PACKAGE: char = '\u{E7B8}';
    pub const HISTORY: char = '\u{E81C}';
    pub const SETTINGS: char = '\u{E713}';
    pub const REFRESH: char = '\u{E72C}';
    pub const DOWNLOAD: char = '\u{E896}';
    pub const CHECK: char = '\u{E73E}';
    pub const WARNING: char = '\u{E7BA}';
    pub const ERROR: char = '\u{E783}';
    pub const INFO: char = '\u{E946}';
    pub const USER: char = '\u{E77B}';
    pub const MORE: char = '\u{E712}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const CANCEL: char = '\u{E711}';
    pub const LOG: char = '\u{E9F9}';
    pub const VIEW: char = '\u{E890}';
    pub const HIDE: char = '\u{ED1A}';
    pub const PLAY: char = '\u{E768}';
    pub const DELETE: char = '\u{E74D}';
    pub const LINK: char = '\u{E71B}';
    pub const HELP: char = '\u{E9CE}';
    pub const ADD: char = '\u{E710}';
    pub const UP: char = '\u{E74A}';
    pub const SIGN_OUT: char = '\u{F3B1}';
}

pub fn icon_text(c: char, size: f32, color: Color32) -> RichText {
    RichText::new(c.to_string()).family(theme::icons()).size(size).color(color)
}

pub fn semibold_text(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text).family(theme::semibold()).size(size).color(color)
}


pub fn soft_mono_text(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text).family(theme::mono_soft_family()).size(size).color(color)
}

pub const RIGHT_GUTTER: f32 = 12.0;

/// Content of a scroll area, inset on the right so the floating scrollbar never covers cards.
pub fn scroll_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin { left: 0, right: RIGHT_GUTTER as i8, top: 0, bottom: 0 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Survey-point marker: a ring with a centre dot, the app's status signature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Quiet,
    Empty,
    Pending,
    Busy,
    Success,
    Error,
}

impl Mark {
    fn color(self) -> Color32 {
        match self {
            Mark::Quiet | Mark::Empty => SUBTLE,
            Mark::Pending | Mark::Busy => ACCENT,
            Mark::Success => SUCCESS,
            Mark::Error => DANGER,
        }
    }
}

pub fn paint_mark(ui: &Ui, center: Pos2, mark: Mark) {
    let color = mark.color();
    let painter = ui.painter();
    painter.circle_stroke(center, 4.5, Stroke::new(1.5_f32, color));
    if mark != Mark::Empty {
        painter.circle_filled(center, 2.0, color);
    }
}

/// Busy variant of the survey-point mark: static centre dot, rotating cobalt arc over a grey ring.
pub fn paint_spinner(ui: &Ui, center: Pos2, radius: f32) {
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(1.5_f32, CONTROL_BORDER));
    let t = ui.input(|i| i.time) as f32;
    let start = t * std::f32::consts::TAU * 0.9;
    let sweep = std::f32::consts::FRAC_PI_2 * 1.2;
    let points: Vec<Pos2> = (0..=16)
        .map(|k| {
            let a = start + sweep * k as f32 / 16.0;
            center + radius * egui::vec2(a.cos(), a.sin())
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(2.0_f32, ACCENT)));
    painter.circle_filled(center, 1.75, ACCENT);
    ui.ctx().request_repaint();
}

pub fn spinner(ui: &mut Ui, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_spinner(ui, rect.center(), size / 2.0 - 1.5);
    resp
}

pub const CHECK_SIZE: f32 = 18.0;
pub const CHECK_GAP: f32 = 8.0;

pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<WidgetText>, enabled: bool) -> Response {
    let galley = text.into().into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
    let height = galley.size().y.max(CHECK_SIZE + 2.0);
    let width = CHECK_SIZE + if galley.size().x > 0.0 { CHECK_GAP + galley.size().x } else { 0.0 };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    if enabled && resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, *checked, galley.text()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let b = Rect::from_center_size(egui::pos2(rect.left() + CHECK_SIZE / 2.0, rect.center().y), Vec2::splat(CHECK_SIZE));
        let hovered = enabled && resp.hovered();
        if *checked {
            let fill = if !enabled { DISABLED_FILL } else if hovered { ACCENT_HOVER } else { ACCENT };
            painter.rect_filled(b, CornerRadius::same(4), fill);
            if !enabled {
                painter.rect_stroke(b, CornerRadius::same(4), Stroke::new(1.0_f32, CONTROL_BORDER), StrokeKind::Inside);
            }
            let ink = if enabled { Color32::WHITE } else { DISABLED_TEXT };
            painter.add(Shape::line(
                vec![
                    b.left_top() + egui::vec2(4.5, 9.3),
                    b.left_top() + egui::vec2(7.6, 12.3),
                    b.left_top() + egui::vec2(13.5, 6.0),
                ],
                Stroke::new(1.75_f32, ink),
            ));
        } else {
            let border = if !enabled { CONTROL_BORDER } else if hovered { MUTED } else { CHECK_BORDER };
            painter.rect_filled(b, CornerRadius::same(4), if enabled { CARD } else { RAISED });
            painter.rect_stroke(b, CornerRadius::same(4), Stroke::new(1.0_f32, border), StrokeKind::Inside);
        }
        if resp.has_focus() {
            painter.rect_stroke(b.expand(2.0), CornerRadius::same(6), Stroke::new(1.5_f32, ACCENT), StrokeKind::Outside);
        }
        let color = if enabled { TEXT } else { DISABLED_TEXT };
        // Centred on the capital-letter height like buttons, so diacritics don't push the text off the box's centre.
        let y = match galley.job.sections.first() {
            Some(s) => {
                let cap = ui.fonts(|f| f.layout_no_wrap("H".to_string(), s.format.font_id.clone(), color));
                let (baseline, cap_top, _) = ink_metrics(&cap);
                b.center().y - (baseline + cap_top / 2.0)
            }
            None => rect.center().y - galley.size().y / 2.0,
        };
        painter.galley(egui::pos2(b.right() + CHECK_GAP, y), galley, color);
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn radio(ui: &mut Ui, selected: bool, title: &str, detail: &str, enabled: bool) -> Response {
    let text_w = (ui.available_width() - CHECK_SIZE - CHECK_GAP).max(40.0);
    let title_g = WidgetText::from(RichText::new(title).size(14.0)).into_galley(ui, Some(TextWrapMode::Truncate), text_w, egui::TextStyle::Body);
    let detail_g = WidgetText::from(RichText::new(detail).size(13.0)).into_galley(ui, Some(TextWrapMode::Wrap), text_w, egui::TextStyle::Body);
    let line_h = title_g.size().y.max(CHECK_SIZE + 2.0);
    let height = line_h + 2.0 + detail_g.size().y;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, mut resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), sense);
    if enabled && resp.clicked() && !selected {
        resp.mark_changed();
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, title_g.text()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = egui::pos2(rect.left() + CHECK_SIZE / 2.0, rect.top() + line_h / 2.0);
        let r = CHECK_SIZE / 2.0;
        let hovered = enabled && resp.hovered();
        if selected {
            let fill = if !enabled { DISABLED_FILL } else if hovered { ACCENT_HOVER } else { ACCENT };
            painter.circle_filled(center, r, fill);
            painter.circle_filled(center, 3.5, if enabled { Color32::WHITE } else { DISABLED_TEXT });
        } else {
            let border = if !enabled { CONTROL_BORDER } else if hovered { MUTED } else { CHECK_BORDER };
            painter.circle(center, r - 0.5, if enabled { CARD } else { RAISED }, Stroke::new(1.0_f32, border));
        }
        if resp.has_focus() {
            painter.circle_stroke(center, r + 3.0, Stroke::new(1.5_f32, ACCENT));
        }
        let x = rect.left() + CHECK_SIZE + CHECK_GAP;
        let title_y = match title_g.job.sections.first() {
            Some(s) => {
                let cap = ui.fonts(|f| f.layout_no_wrap("H".to_string(), s.format.font_id.clone(), TEXT));
                let (baseline, cap_top, _) = ink_metrics(&cap);
                center.y - (baseline + cap_top / 2.0)
            }
            None => center.y - title_g.size().y / 2.0,
        };
        painter.galley(egui::pos2(x, title_y), title_g, if enabled { TEXT } else { DISABLED_TEXT });
        painter.galley(egui::pos2(x, rect.top() + line_h + 2.0), detail_g, if enabled { MUTED } else { DISABLED_TEXT });
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn language_picker(ui: &mut Ui, current: Lang) -> Option<Lang> {
    let mut picked = None;
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
        egui::ComboBox::from_id_salt("language")
            .width(120.0)
            .icon(chevron)
            .selected_text(current.name())
            .show_ui(ui, |ui| {
                for lang in Lang::ALL {
                    if ui.selectable_label(lang == current, lang.name()).clicked() && lang != current {
                        picked = Some(lang);
                    }
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("{}: {}", t!("lang.label"), current.name())));
    });
    picked
}

/// ComboBox icon: a small chevron instead of egui's filled triangle.
pub fn chevron(ui: &Ui, rect: Rect, _visuals: &egui::style::WidgetVisuals, open: bool, _above: egui::AboveOrBelow) {
    let c = rect.center();
    let (w, h) = (4.0, 2.25);
    let dir = if open { -1.0 } else { 1.0 };
    let color = if ui.is_enabled() { MUTED } else { DISABLED_TEXT };
    ui.painter().add(Shape::line(
        vec![c + egui::vec2(-w, -h * dir), c + egui::vec2(0.0, h * dir), c + egui::vec2(w, -h * dir)],
        Stroke::new(1.5_f32, color),
    ));
}

/// Baseline and ink extent (relative to the baseline) of a single-line galley.
fn ink_metrics(galley: &egui::Galley) -> (f32, f32, f32) {
    let glyphs = galley.rows.iter().flat_map(|r| r.glyphs.iter()).filter(|g| g.uv_rect.size.y > 0.0);
    let baseline = galley.rows.first().and_then(|r| r.glyphs.first()).map_or(0.0, |g| g.pos.y);
    let (mut top, mut bottom) = (0.0_f32, 0.0_f32);
    for g in glyphs {
        top = top.min(g.pos.y + g.uv_rect.offset.y - baseline);
        bottom = bottom.max(g.pos.y + g.uv_rect.offset.y + g.uv_rect.size.y - baseline);
    }
    (baseline, top, bottom)
}

/// Top and bottom of a single-glyph galley's visible ink, relative to the galley top.
/// Read from the rasterised glyph because icon fonts report loose outline bounds.
fn icon_ink(ui: &Ui, galley: &egui::Galley) -> (f32, f32) {
    type InkSpan = ([u16; 2], [u16; 2], f32, f32);
    static CACHE: std::sync::Mutex<Vec<InkSpan>> = std::sync::Mutex::new(Vec::new());
    let Some(g) = galley.rows.first().and_then(|r| r.glyphs.first()) else {
        return (0.0, galley.size().y);
    };
    let uv = g.uv_rect;
    let top = g.pos.y + uv.offset.y;
    let rows = uv.max[1].saturating_sub(uv.min[1]);
    if rows == 0 {
        return (top, top + uv.size.y);
    }
    let scale = uv.size.y / rows as f32;
    let cached = CACHE.lock().ok().and_then(|c| c.iter().find(|e| e.0 == uv.min && e.1 == uv.max).map(|e| (e.2, e.3)));
    let (first, last) = cached.unwrap_or_else(|| {
        let image = ui.fonts(|f| f.image());
        let width = image.size[0];
        let inked = |y: u16| (uv.min[0]..uv.max[0]).any(|x| image.pixels.get(y as usize * width + x as usize).is_some_and(|&c| c > 0.25));
        let first = (uv.min[1]..uv.max[1]).find(|&y| inked(y)).unwrap_or(uv.min[1]);
        let last = (uv.min[1]..uv.max[1]).rev().find(|&y| inked(y)).unwrap_or(uv.max[1] - 1);
        let span = ((first - uv.min[1]) as f32, (last + 1 - uv.min[1]) as f32);
        if let Ok(mut c) = CACHE.lock() {
            c.push((uv.min, uv.max, span.0, span.1));
        }
        span
    });
    (top + first * scale, top + last * scale)
}

/// The "→" between two values, a little heavier than the text so it does not look faint next to the digits.
fn append_arrow(job: &mut LayoutJob, size: f32, color: Color32) {
    job.append("→", 0.0, TextFormat { font_id: FontId::new(size + 1.0, theme::semibold()), color, ..Default::default() });
    job.append(" ", 0.0, TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, ..Default::default() });
}

/// Grey box of "name   from → to" rows, padded by the letters' ink so the top and bottom margins match.
pub fn change_list(ui: &mut Ui, lines: &[(String, String)]) {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 12.0;
    const PITCH: f32 = 26.0;
    const NAME_COL: f32 = 160.0;
    let rows: Vec<_> = lines
        .iter()
        .map(|(name, change)| {
            let name = WidgetText::from(semibold_text(name, 13.5, TEXT)).into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
            let mut job = LayoutJob::default();
            let (from, to) = change.split_once(" → ").map_or((change.as_str(), ""), |(a, b)| (a, b));
            job.append(from, 0.0, TextFormat { font_id: FontId::new(13.5, FontFamily::Proportional), color: MUTED, ..Default::default() });
            if !to.is_empty() {
                job.append(" ", 0.0, TextFormat { font_id: FontId::new(13.5, FontFamily::Proportional), color: MUTED, ..Default::default() });
                append_arrow(&mut job, 13.5, MUTED);
                job.append(to, 0.0, TextFormat { font_id: FontId::new(13.5, theme::semibold()), color: TEXT, ..Default::default() });
            }
            let change = WidgetText::from(job).into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
            (name, change)
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    let extent = |i: usize| {
        let (_, t1, b1) = ink_metrics(&rows[i].0);
        let (_, t2, b2) = ink_metrics(&rows[i].1);
        (t1.min(t2), b1.max(b2))
    };
    let (top, _) = extent(0);
    let (_, bottom) = extent(rows.len() - 1);
    let height = PAD_Y * 2.0 + (rows.len() - 1) as f32 * PITCH + bottom - top;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height.round()), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), RAISED);
    for (i, (name, change)) in rows.into_iter().enumerate() {
        let baseline = rect.top() + PAD_Y - top + i as f32 * PITCH;
        let (nb, _, _) = ink_metrics(&name);
        let (cb, _, _) = ink_metrics(&change);
        painter.galley(egui::pos2(rect.left() + PAD_X, baseline - nb), name, TEXT);
        painter.galley(egui::pos2(rect.left() + PAD_X + NAME_COL, baseline - cb), change, MUTED);
    }
}


pub fn button_width(ui: &Ui, ic: Option<char>, text: &str) -> f32 {
    let mut job = LayoutJob::default();
    if let Some(c) = ic {
        job.append(&c.to_string(), 0.0, TextFormat { font_id: FontId::new(14.0, theme::icons()), ..Default::default() });
    }
    let leading = if ic.is_some() && !text.is_empty() { 8.0 } else { 0.0 };
    job.append(text, leading, TextFormat { font_id: FontId::new(14.0, FontFamily::Proportional), ..Default::default() });
    let width = ui.fonts(|f| f.layout_job(job)).size().x;
    let padding = if text.is_empty() { 8.0 } else { 16.0 };
    (width + padding * 2.0).max(32.0)
}

pub fn mark_label(ui: &mut Ui, mark: Mark, text: &str, size: f32) -> Response {
    let color = match mark {
        Mark::Quiet | Mark::Empty => MUTED,
        other => other.color(),
    };
    let rtl = ui.layout().prefer_right_to_left();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let text = RichText::new(text).size(size).color(color);
        let label = rtl.then(|| ui.label(text.clone()));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, size + 4.0), Sense::hover());
        if mark == Mark::Busy {
            paint_spinner(ui, rect.center(), 5.0);
        } else {
            paint_mark(ui, rect.center(), mark);
        }
        label.unwrap_or_else(|| ui.label(text))
    })
    .inner
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(ui: &mut Ui, ic: Option<char>, text: &str, kind: Btn, enabled: bool) -> Response {
    button_sized(ui, ic, text, kind, enabled, 0.0)
}

pub fn button_sized(ui: &mut Ui, ic: Option<char>, text: &str, kind: Btn, enabled: bool, min_width: f32) -> Response {
    button_ex(ui, ic, text, kind, enabled, min_width, 32.0)
}

pub fn button_ex(ui: &mut Ui, ic: Option<char>, text: &str, kind: Btn, enabled: bool, min_width: f32, height: f32) -> Response {
    let (bg, bg_hover, fg, border) = match kind {
        Btn::Primary => (ACCENT, ACCENT_HOVER, Color32::WHITE, Color32::TRANSPARENT),
        Btn::Secondary => (CARD, RAISED, TEXT, CONTROL_BORDER),
        Btn::Ghost => (Color32::TRANSPARENT, HOVER, MUTED, Color32::TRANSPARENT),
        Btn::Danger => (DANGER, Color32::from_rgb(0xB9, 0x1C, 0x1C), Color32::WHITE, Color32::TRANSPARENT),
    };
    let fg = if enabled { fg } else { DISABLED_TEXT };

    let text_font = FontId::new(14.0, FontFamily::Proportional);
    let icon_color = if kind == Btn::Secondary && enabled { MUTED } else { fg };
    let (text_galley, cap_galley, icon_galley) = ui.fonts(|f| {
        (
            f.layout_no_wrap(text.to_string(), text_font.clone(), fg),
            f.layout_no_wrap("H".to_string(), text_font.clone(), fg),
            ic.map(|c| f.layout_no_wrap(c.to_string(), FontId::new(if text.is_empty() { 16.0 } else { 14.0 }, theme::icons()), icon_color)),
        )
    });
    let gap = if icon_galley.is_some() && !text.is_empty() { 8.0 } else { 0.0 };
    let icon_w = icon_galley.as_ref().map_or(0.0, |g| g.size().x);
    let text_w = if text.is_empty() { 0.0 } else { text_galley.size().x };
    let content_w = icon_w + gap + text_w;
    let padding = if text.is_empty() { 8.0 } else { 16.0 };
    let size = egui::vec2((content_w + padding * 2.0).max(min_width).max(height), height);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let pressed = enabled && response.is_pointer_button_down_on();
        let mut fill = if hovered { bg_hover } else { bg };
        if !enabled && matches!(kind, Btn::Primary | Btn::Danger) {
            fill = DISABLED_FILL;
        } else if !enabled && kind == Btn::Secondary {
            fill = RAISED;
        }
        let rect_drawn = if pressed { rect.shrink(0.5) } else { rect };
        let painter = ui.painter();
        painter.rect_filled(rect_drawn, CornerRadius::same(8), fill);
        if border != Color32::TRANSPARENT {
            painter.rect_stroke(rect_drawn, CornerRadius::same(8), Stroke::new(1.0_f32, border), StrokeKind::Inside);
        }
        if response.has_focus() {
            painter.rect_stroke(rect.expand(2.0), CornerRadius::same(10), Stroke::new(1.5_f32, ACCENT), StrokeKind::Outside);
        }
        // Text and icon are centred on the capital-letter height, so diacritics and the icon font's own metrics don't shift them.
        let (baseline, cap_top, _) = ink_metrics(&cap_galley);
        let mid = rect.center().y;
        let left = rect.center().x - content_w / 2.0;
        if let Some(g) = icon_galley {
            let (itop, ibottom) = icon_ink(ui, &g);
            let y = mid - (itop + ibottom) / 2.0;
            painter.galley(egui::pos2(left, y), g, icon_color);
        }
        if !text.is_empty() {
            let y = mid - (baseline + cap_top / 2.0);
            painter.galley(egui::pos2(left + icon_w + gap, y), text_galley, fg);
        }
    }
    if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_with(ui, Margin::same(20), add)
}

/// A card made of 32px rows: titled cards start with the 22px title row, so a shorter bottom margin puts the last
/// row's centre as far from the bottom edge as the title's centre is from the top.
pub fn row_card<R>(ui: &mut Ui, titled: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    let top = if titled { 20 } else { 15 };
    card_with(ui, Margin { left: 20, right: 20, top, bottom: 15 }, add)
}

pub fn card_with<R>(ui: &mut Ui, margin: Margin, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn card_title(ui: &mut Ui, title: &str) {
    card_header(ui, title, |_| {});
}

/// Card title row of a fixed height, with optional controls on the right, so every card's content starts at the same offset.
pub fn card_header(ui: &mut Ui, title: &str, right: impl FnOnce(&mut Ui)) {
    let w = ui.available_width();
    // Controls on the right may be taller than the title; they overhang the row instead of pushing the content down.
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 22.0), Sense::hover());
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.expand2(egui::vec2(0.0, 10.0))).layout(Layout::left_to_right(Align::Center)));
    row.label(semibold_text(title, 15.0, TEXT));
    row.with_layout(Layout::right_to_left(Align::Center), right);
}

/// "lead · detail": the lead carries the state colour, the detail stays secondary. Only Busy gets a glyph (the spinner).
fn status_line(ui: &mut Ui, mark: Mark, text: &str) {
    let (lead, rest) = text.split_once(" · ").map_or((text, ""), |(a, b)| (a, b));
    let (color, family) = match mark {
        Mark::Quiet | Mark::Empty => (MUTED, FontFamily::Proportional),
        Mark::Busy => (ACCENT, theme::semibold()),
        Mark::Pending => (TEXT, theme::semibold()),
        other => (other.color(), theme::semibold()),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if mark == Mark::Busy {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), Sense::hover());
            paint_spinner(ui, rect.center(), 5.0);
        }
        let mut job = LayoutJob::default();
        job.append(lead, 0.0, TextFormat { font_id: FontId::new(13.5, family), color, ..Default::default() });
        if !rest.is_empty() {
            job.append(&format!(" · {rest}"), 0.0, TextFormat { font_id: FontId::new(13.5, FontFamily::Proportional), color: MUTED, ..Default::default() });
        }
        ui.label(job);
    });
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, mark: Option<Mark>, right: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.set_width(ui.available_width() - RIGHT_GUTTER);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(RichText::new(title).size(28.0).color(TEXT));
            if !subtitle.is_empty() {
                match mark {
                    Some(m) => {
                        status_line(ui, m, subtitle);
                    }
                    None => status_line(ui, Mark::Quiet, subtitle),
                }
            }
        });
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 38.0), Layout::right_to_left(Align::Center), right);
    });
    ui.add_space(20.0);
}

#[derive(Clone, Copy)]
pub enum Tone {
    Info,
    Success,
    Warn,
    Danger,
}

impl Tone {
    fn style(self) -> (Color32, Color32, Color32, char) {
        match self {
            Tone::Info => (ACCENT, CARD, BORDER, icon::INFO),
            Tone::Success => (SUCCESS, CARD, BORDER, icon::CHECK),
            Tone::Warn => (WARN, WARN_SOFT, WARN_BORDER, icon::WARNING),
            Tone::Danger => (DANGER, DANGER_SOFT, DANGER_BORDER, icon::ERROR),
        }
    }
}

pub fn banner(ui: &mut Ui, tone: Tone, title: &str, text: &str, right_width: f32, right: impl FnOnce(&mut Ui)) {
    let (fg, bg, border, ic) = tone.style();
    Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.allocate_ui_with_layout(egui::vec2(16.0, 20.0), Layout::top_down(Align::Center), |ui| {
                    ui.add_space(2.0);
                    ui.label(icon_text(ic, 16.0, fg));
                });
                let text_width = (ui.available_width() - right_width).max(120.0);
                ui.allocate_ui_with_layout(egui::vec2(text_width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(semibold_text(title, 14.0, TEXT));
                    if !text.is_empty() {
                        ui.add(egui::Label::new(path_job(text, 13.0, MUTED)).wrap());
                    }
                });
                if right_width > 0.0 {
                    ui.with_layout(Layout::right_to_left(Align::Min), right);
                }
            });
        });
    ui.add_space(12.0);
}

pub fn indented<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        Frame::new()
            .inner_margin(Margin { left: (CHECK_SIZE + CHECK_GAP) as i8, right: 0, top: 0, bottom: 4 })
            .show(ui, add)
            .inner
    })
    .inner
}

pub fn module_tile(ui: &mut Ui, text: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), RAISED);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::new(12.5, theme::semibold()),
        MUTED,
    );
}

pub fn progress_bar(ui: &mut Ui, fraction: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), TRACK);
    match fraction {
        Some(f) => {
            let mut r = rect;
            r.set_width((rect.width() * f.clamp(0.0, 1.0)).max(6.0));
            painter.rect_filled(r, CornerRadius::same(3), ACCENT);
        }
        None => {
            let time = ui.input(|i| i.time);
            let w = rect.width() * 0.28;
            let t = (time * 0.55).fract() as f32;
            let x = rect.left() - w + (rect.width() + w) * t;
            let r = Rect::from_min_max(
                egui::pos2(x.max(rect.left()), rect.top()),
                egui::pos2((x + w).min(rect.right()), rect.bottom()),
            );
            if r.width() > 0.0 {
                painter.rect_filled(r, CornerRadius::same(3), ACCENT);
            }
            ui.ctx().request_repaint();
        }
    }
}

/// Sidebar row content: icon 12px from the left, label 40px from the left, both centred vertically on their
/// visible ink (font line metrics would put the icon and accented capitals off-centre).
pub fn icon_label(ui: &Ui, rect: Rect, ic: char, label: &str, active: bool, max_width: f32) {
    let (icon_color, color) = if active { (TEXT, TEXT) } else { (SUBTLE, MUTED) };
    let font = FontId::new(14.0, if active { theme::semibold() } else { FontFamily::Proportional });
    let mut job = LayoutJob::simple_singleline(label.to_string(), font.clone(), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    let (icon_g, text_g, cap_g) = ui.fonts(|f| {
        (
            f.layout_no_wrap(ic.to_string(), FontId::new(16.0, theme::icons()), icon_color),
            f.layout_job(job),
            f.layout_no_wrap("H".to_string(), font, color),
        )
    });
    let (itop, ibottom) = icon_ink(ui, &icon_g);
    let (baseline, cap_top, _) = ink_metrics(&cap_g);
    let mid = rect.center().y;
    let painter = ui.painter();
    painter.galley(egui::pos2(rect.left() + 12.0, mid - (itop + ibottom) / 2.0), icon_g, icon_color);
    painter.galley(egui::pos2(rect.left() + 40.0, mid - (baseline + cap_top / 2.0)), text_g, color);
}

pub fn nav_item(ui: &mut Ui, ic: char, label: &str, active: bool, count: usize) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label));
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, CornerRadius::same(8), CARD);
        painter.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        let bar = Rect::from_min_size(egui::pos2(rect.left() + 1.0, rect.top() + 8.0), egui::vec2(3.0, rect.height() - 16.0));
        painter.rect_filled(bar, CornerRadius::same(2), ACCENT);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(8), HOVER);
    }
    icon_label(ui, rect, ic, label, active, f32::INFINITY);
    let painter = ui.painter();
    if count > 0 {
        let galley = painter.layout_no_wrap(count.to_string(), FontId::new(12.0, theme::semibold()), Color32::WHITE);
        let w = (galley.size().x + 12.0).max(20.0);
        let pill = Rect::from_center_size(egui::pos2(rect.right() - 12.0 - w / 2.0, rect.center().y), egui::vec2(w, 20.0));
        painter.rect_filled(pill, CornerRadius::same(10), ACCENT);
        painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A 32px "⋯" button that opens a popup menu below it.
pub fn more_menu(ui: &mut Ui, id_salt: impl std::hash::Hash, name: &str, enabled: bool, open: bool, add: impl FnOnce(&mut Ui)) {
    let resp = button(ui, Some(icon::MORE), "", Btn::Secondary, enabled).on_hover_text(t!("widgets.more_actions"));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, name));
    if open && enabled {
        ui.memory_mut(|m| m.open_popup(ui.make_persistent_id(&id_salt)));
    }
    attach_menu(ui, id_salt, resp, add);
}

/// Secondary button with a chevron that opens a popup menu below it.
pub fn menu_button(ui: &mut Ui, id_salt: impl std::hash::Hash, ic: Option<char>, label: &str, add: impl FnOnce(&mut Ui)) {
    let resp = button(ui, ic, &format!("{label}   "), Btn::Secondary, true);
    let c = egui::pos2(resp.rect.right() - 16.0, resp.rect.center().y);
    ui.painter().add(Shape::line(
        vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(0.0, 2.0), c + egui::vec2(4.0, -2.0)],
        Stroke::new(1.5_f32, MUTED),
    ));
    attach_menu(ui, id_salt, resp, add);
}

const MENU_WIDTH: f32 = 232.0;

fn attach_menu(ui: &mut Ui, id_salt: impl std::hash::Hash, resp: Response, add: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(id_salt);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    if !ui.memory(|m| m.is_popup_open(id)) {
        return;
    }
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(resp.rect.right_bottom() + egui::vec2(0.0, 4.0))
        .pivot(egui::Align2::RIGHT_TOP)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).inner_margin(Margin::same(6)).corner_radius(CornerRadius::same(12)).show(ui, |ui| {
                ui.set_width(MENU_WIDTH);
                ui.spacing_mut().item_spacing.y = 2.0;
                add(ui);
            });
        });
    let dismiss = ui.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.any_click());
    if dismiss && !resp.clicked() {
        ui.memory_mut(|m| m.close_popup());
    }
}

/// Row inside a popup menu. Disabled rows stay hoverable so the caller can explain why.
pub fn menu_item(ui: &mut Ui, ic: Option<char>, label: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 34.0), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if enabled && resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(6), HOVER);
        }
        let mut x = rect.left() + 12.0;
        if let Some(c) = ic {
            let color = if enabled { MUTED } else { DISABLED_TEXT };
            painter.text(egui::pos2(x + 8.0, rect.center().y), egui::Align2::CENTER_CENTER, c.to_string(), FontId::new(14.0, theme::icons()), color);
            x += 28.0;
        }
        let color = if enabled { TEXT } else { DISABLED_TEXT };
        painter.text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, label, FontId::new(14.0, FontFamily::Proportional), color);
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 9.0), Sense::hover());
    ui.painter().hline(rect.x_range().shrink(4.0), rect.center().y.round() + 0.5, Stroke::new(1.0_f32, BORDER));
}

fn is_path(word: &str) -> bool {
    const EXT: [&str; 8] = ["zip", "msi", "exe", "bat", "txt", "log", "ini", "cfg"];
    let parts: Vec<&str> = word.split('.').collect();
    let date = parts.len() == 3 && parts[2].len() == 4;
    let version = !date && parts.len() >= 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    version || word.contains('\\') || word.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && EXT.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The text with file names and paths left out, e.g. "Stahování soubor.zip" becomes "Stahování".
pub fn without_paths(text: &str) -> String {
    text.split(' ').filter(|w| !is_path(w.trim_matches(|c: char| "()\"„“.,;:".contains(c)))).collect::<Vec<_>>().join(" ")
}

/// Segoe UI digits are tabular, so numbers that update in place don't jitter.
pub fn numbers_job(text: &str, size: f32, color: Color32) -> LayoutJob {
    LayoutJob::single_section(text.to_string(), TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, ..Default::default() })
}

pub fn path_job(text: &str, size: f32, color: Color32) -> LayoutJob {
    LayoutJob::single_section(text.to_string(), TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color, ..Default::default() })
}

pub fn muted(ui: &mut Ui, text: &str) {
    let mut job = path_job(text, 13.5, MUTED);
    job.wrap.max_width = ui.available_width().min(640.0);
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

/// Vertical page scroll with the right gutter and a hairline under the header once content scrolls.
pub fn scroll_page<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| scroll_body(ui, add));
    let r = out.inner_rect;
    let painter = ui.painter().with_clip_rect(r.expand(8.0));
    let (left, right) = (r.left() - 4.0, r.right());
    let hidden_below = out.content_size.y - r.height() - out.state.offset.y;
    if out.state.offset.y > 0.5 {
        painter.rect_filled(Rect::from_min_max(egui::pos2(left, r.top() - 6.0), egui::pos2(right, r.top() + 1.0)), 0.0, BG);
        paint_fade(&painter, Rect::from_min_max(egui::pos2(left, r.top() + 1.0), egui::pos2(right, r.top() + 17.0)), true);
        painter.hline(r.left()..=r.right() - RIGHT_GUTTER, r.top() + 0.5, Stroke::new(1.0_f32, BORDER));
    }
    if hidden_below > 0.5 {
        painter.rect_filled(Rect::from_min_max(egui::pos2(left, r.bottom()), egui::pos2(right, r.bottom() + 6.0)), 0.0, BG);
        paint_fade(&painter, Rect::from_min_max(egui::pos2(left, r.bottom() - 24.0), egui::pos2(right, r.bottom())), false);
    }
    out.inner
}

fn paint_fade(painter: &egui::Painter, rect: Rect, solid_top: bool) {
    let clear = Color32::from_rgba_unmultiplied(BG.r(), BG.g(), BG.b(), 0);
    let (top, bottom) = if solid_top { (BG, clear) } else { (clear, BG) };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}
