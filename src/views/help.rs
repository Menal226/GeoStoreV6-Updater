use crate::app::App;
use crate::i18n;
use crate::local;
use crate::t;
use crate::theme::{self, *};
use crate::widgets::{self, icon, Btn};
use eframe::egui::{self, text::LayoutJob, Align, FontId, Layout, RichText, TextFormat};

const MAX_PAGE: f32 = 800.0;
const KEY_COL: f32 = 260.0;
const CONTACT: &str = "gsupdater@menal.net";

enum Block {
    P(&'static str),
    Li(&'static str),
    Row(&'static str, &'static str),
    Q(&'static str, &'static str),
}

use Block::*;

const SECTIONS: &[(&str, &[Block])] = &[
    (
        "help.overview.title",
        &[
            P("help.overview.p1"),
            Li("help.overview.li1"),
            Li("help.overview.li2"),
            Li("help.overview.li3"),
            P("help.overview.p2"),
            P("help.overview.p3"),
            Li("help.overview.li4"),
            Li("help.overview.li5"),
            Li("help.overview.li6"),
            P("help.overview.p4"),
        ],
    ),
    (
        "help.updates.title",
        &[
            P("help.updates.p1"),
            Li("help.updates.li1"),
            Li("help.updates.li2"),
            Li("help.updates.li3"),
            Li("help.updates.li4"),
        ],
    ),
    (
        "help.new_install.title",
        &[
            P("help.new_install.p1"),
            Li("help.new_install.li1"),
            Li("help.new_install.li2"),
            Li("help.new_install.li3"),
            Li("help.new_install.li4"),
            Li("help.new_install.li5"),
            P("help.new_install.p2"),
        ],
    ),
    (
        "help.history.title",
        &[
            P("help.history.p1"),
            Li("help.history.li1"),
            Li("help.history.li2"),
            Li("help.history.li3"),
        ],
    ),
    (
        "help.settings.title",
        &[
            Li("help.settings.li1"),
            Li("help.settings.li2"),
            Li("help.settings.li3"),
            Li("help.settings.li4"),
            Li("help.settings.li5"),
            Li("help.settings.li6"),
        ],
    ),
    (
        "help.background.title",
        &[
            P("help.background.p1"),
            Li("help.background.li1"),
            Li("help.background.li2"),
            Li("help.background.li3"),
            P("help.background.p2"),
        ],
    ),
    (
        "help.data.title",
        &[
            Row("help.data.settings", "C:\\ProgramData\\GeoStore Updater"),
            Row("help.data.backups", "C:\\V6-KONFIG_zalohy"),
            Row("help.data.credentials", "GeoStoreUpdater/portal.geostore.cz"),
            Row("help.data.licence", "C:\\Program Files\\GEOVAP\\GeoStoreV6\\gsfl.lic"),
        ],
    ),
    (
        "help.uninstall.title",
        &[
            P("help.uninstall.p1"),
            P("help.uninstall.p2"),
        ],
    ),
    (
        "help.troubleshooting.title",
        &[
            Q("help.troubleshooting.q1", "help.troubleshooting.a1"),
            Q("help.troubleshooting.q2", "help.troubleshooting.a2"),
            Q("help.troubleshooting.q3", "help.troubleshooting.a3"),
            Q("help.troubleshooting.q4", "help.troubleshooting.a4"),
            Q("help.troubleshooting.q5", "help.troubleshooting.a5"),
            Q("help.troubleshooting.q6", "help.troubleshooting.a6"),
        ],
    ),
    (
        "help.legal.title",
        &[
            P("help.legal.p1"),
            P("help.legal.p2"),
            P("help.legal.p3"),
            P("help.legal.p4"),
        ],
    ),
];

/// Text with **bold** runs; paths and file names in the rest are set in mono.
fn rich(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, part) in text.split("**").enumerate() {
        if part.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            job.append(part, 0.0, TextFormat { font_id: FontId::new(14.0, theme::semibold()), color: TEXT, ..Default::default() });
        } else {
            let plain = widgets::path_job(part, 14.0, TEXT);
            for s in &plain.sections {
                job.append(&plain.text[s.byte_range.clone()], 0.0, s.format.clone());
            }
        }
    }
    job
}

fn paragraph(ui: &mut egui::Ui, text: &str) {
    let mut job = rich(text);
    for s in &mut job.sections {
        s.format.line_height = Some(21.0);
    }
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

fn block(ui: &mut egui::Ui, b: &Block) {
    match b {
        P(key) => paragraph(ui, i18n::get(key)),
        Li(key) => {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_sized([18.0, 20.0], egui::Label::new(RichText::new("•").size(14.0).color(SUBTLE)));
                paragraph(ui, i18n::get(key));
            });
        }
        Row(name, value) => {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                ui.allocate_ui_with_layout(egui::vec2(KEY_COL, 24.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(KEY_COL);
                    ui.label(RichText::new(i18n::get(name)).size(14.0).color(MUTED));
                });
                ui.label(widgets::soft_mono_text(*value, 13.0, MONO_INK));
            });
        }
        Q(question, answer) => {
            ui.label(widgets::semibold_text(i18n::get(question), 14.0, TEXT));
            ui.add_space(2.0);
            paragraph(ui, i18n::get(answer));
        }
    }
}

fn gap(prev: &Block, next: &Block) -> f32 {
    match (prev, next) {
        (Li(_), Li(_)) | (Row(..), Row(..)) => 4.0,
        (Q(..), Q(..)) => 14.0,
        _ => 10.0,
    }
}

impl App {
    pub(crate) fn ui_help(&mut self, ui: &mut egui::Ui) {
        widgets::page_header(ui, t!("help.title"), "", None, |ui| {
            if widgets::button(ui, Some(icon::LINK), t!("help.github_guide"), Btn::Secondary, true).clicked() {
                local::open_url(&format!("{}#readme", super::settings::REPO_URL));
            }
        });

        widgets::scroll_page(ui, "help_scroll", |ui| {
            ui.set_max_width(ui.available_width().min(MAX_PAGE));
            egui::Frame::new()
                .fill(CARD)
                .stroke(egui::Stroke::new(1.0_f32, BORDER))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(20, 0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, (title, blocks)) in SECTIONS.iter().enumerate() {
                        if i > 0 {
                            let y = ui.cursor().top();
                            ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0_f32, BORDER));
                        }
                        let id = egui::Id::new(("help_open", *title));
                        let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
                        if section_header(ui, i18n::get(title), open) {
                            open = !open;
                        }
                        if open {
                            for (j, b) in blocks.iter().enumerate() {
                                if j > 0 {
                                    ui.add_space(gap(&blocks[j - 1], b));
                                }
                                block(ui, b);
                            }
                            ui.add_space(18.0);
                        }
                        ui.data_mut(|d| d.insert_temp(id, open));
                    }
                });
            ui.add_space(12.0);

            widgets::card(ui, |ui| {
                widgets::card_title(ui, t!("help.contact.title"));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.label(RichText::new(t!("help.contact.text")).size(14.0).color(TEXT));
                    ui.label(widgets::soft_mono_text(CONTACT, 13.0, MONO_INK));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, None, t!("help.contact.write_email"), Btn::Secondary, true).clicked() {
                            local::open_url(&format!("mailto:{CONTACT}"));
                        }
                    });
                });
            });
            ui.add_space(28.0);
        });
    }
}

/// A topic row that opens and closes its text in place. Returns true when clicked.
fn section_header(ui: &mut egui::Ui, title: &str, open: bool) -> bool {
    let full = ui.max_rect().x_range();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 48.0), egui::Sense::hover());
    let hit = egui::Rect::from_x_y_ranges(full.min - 20.0..=full.max + 20.0, rect.y_range());
    let resp = ui.interact(hit, ui.id().with(("help_section", title)), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, title));
    let painter = ui.painter();
    painter.text(rect.left_center(), egui::Align2::LEFT_CENTER, title, FontId::new(15.0, theme::semibold()), TEXT);
    let c = egui::pos2(rect.right() - 6.0, rect.center().y);
    let color = if resp.hovered() { TEXT } else { MUTED };
    let dy = if open { -1.0 } else { 1.0 };
    let points = vec![c + egui::vec2(-5.0, -2.5 * dy), c + egui::vec2(0.0, 2.5 * dy), c + egui::vec2(5.0, -2.5 * dy)];
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    if resp.has_focus() {
        painter.rect_stroke(hit.shrink(2.0), egui::CornerRadius::same(8), egui::Stroke::new(1.5_f32, ACCENT), egui::StrokeKind::Inside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}
