use crate::app::App;
use crate::i18n;
use crate::local;
use crate::t;
use crate::theme::{self, *};
use crate::widgets::{self, icon, Btn};
use eframe::egui::{self, text::LayoutJob, Align, FontId, Layout, RichText, TextFormat};

const MAX_TEXT: f32 = 720.0;
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
    job.wrap.max_width = ui.available_width().min(MAX_TEXT);
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
            for (title, blocks) in SECTIONS {
                widgets::card(ui, |ui| {
                    widgets::card_title(ui, i18n::get(title));
                    ui.add_space(12.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, b) in blocks.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(gap(&blocks[i - 1], b));
                        }
                        block(ui, b);
                    }
                });
                ui.add_space(12.0);
            }

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
