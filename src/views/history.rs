use crate::app::App;
use crate::fmt;
use crate::i18n;
use crate::logfile::{self, Level};
use crate::paths;
use crate::t;
use crate::theme::*;
use crate::widgets::{self, icon, Mark};
use chrono::{Local, NaiveDate};
use eframe::egui::{self, text::LayoutJob, Align, FontFamily, FontId, Layout, RichText, Sense, TextFormat};

enum Entry {
    Day(NaiveDate),
    Line { newest: usize, oldest: usize, text: String, level: Level, repeat: usize, resolved: bool },
}

fn day_label(d: NaiveDate) -> String {
    let today = Local::now().date_naive();
    if d == today {
        t!("history.today", date = fmt::date(d))
    } else if Some(d) == today.pred_opt() {
        t!("history.yesterday", date = fmt::date(d))
    } else {
        fmt::date(d)
    }
}

fn all_up_to_date(text: &str) -> bool {
    i18n::Lang::ALL.iter().any(|&lang| {
        let full = i18n::fill(i18n::get_in(lang, "log.check_done"), &[("outcome", i18n::get_in(lang, "log.all_up_to_date").to_string())]);
        let prefix = full.split('{').next().unwrap_or_default();
        !prefix.is_empty() && text.starts_with(prefix)
    })
}

/// Current wording for a log line (older log files used different phrasing); routine lines become Detail.
fn normalize(level: Level, text: &str) -> (Level, String) {
    let mut text = text.lines().next().unwrap_or_default().to_string();
    if let Some(rest) = text.strip_prefix("Kontrola aktualizací dokončena: ") {
        let nums: Vec<usize> = rest
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|s| s.parse().ok())
            .collect();
        if let [total, pending] = nums[..] {
            let outcome = match pending {
                0 => t!("log.all_up_to_date").to_string(),
                _ => i18n::fill(i18n::plural("log.updates_available", pending as u64), &[("n", pending.to_string())]),
            };
            text = t!("log.check_done", outcome = outcome, count = total, packages = fmt::packages(total));
        }
    }
    let routine = i18n::starts_with_any(&text, "log.logged_in") || all_up_to_date(&text);
    let level = if routine && level == Level::Info { Level::Detail } else { level };
    (level, text.trim_end_matches('.').to_string())
}

impl App {
    pub(crate) fn ui_history(&mut self, ui: &mut egui::Ui) {
        widgets::page_header(ui, t!("history.title"), "", None, |ui| {
            widgets::menu_button(ui, "history_menu", Some(icon::LOG), t!("history.logs"), |ui| {
                if widgets::menu_item(ui, Some(icon::LOG), t!("history.app_log"), true).clicked() {
                    App::open(logfile::path());
                }
                let davka = paths::davka_log();
                let item = widgets::menu_item(ui, Some(icon::LOG), t!("history.installer_log"), davka.is_file());
                if !davka.is_file() {
                    item.on_hover_text(t!("history.no_installer_log"));
                } else if item.clicked() {
                    App::open(davka);
                }
                widgets::menu_separator(ui);
                if widgets::menu_item(ui, Some(icon::FOLDER), t!("history.open_backups"), true).clicked() {
                    App::open(paths::BACKUP_ROOT);
                }
            });
        });

        let mut entries: Vec<Entry> = Vec::new();
        let mut current: Option<NaiveDate> = None;
        let mut later_success = false;
        for (i, l) in self.log.iter().enumerate().rev() {
            let (level, text) = normalize(l.level, &l.text);
            let resolved = level == Level::Error && later_success;
            later_success |= l.level == Level::Success || i18n::starts_with_any(&text, "log.check_done");
            if !self.show_detail && level == Level::Detail {
                continue;
            }
            let d = l.time.date_naive();
            if current != Some(d) {
                current = Some(d);
                entries.push(Entry::Day(d));
            } else if let Some(Entry::Line { text: prev, level: prev_level, repeat, oldest, .. }) = entries.last_mut() {
                if *prev == text && *prev_level == level && !l.text.contains('\n') {
                    *repeat += 1;
                    *oldest = i;
                    continue;
                }
            }
            entries.push(Entry::Line { newest: i, oldest: i, text, level, repeat: 1, resolved });
        }

        let routine = self.log.iter().filter(|l| normalize(l.level, &l.text).0 == Level::Detail).count();

        widgets::scroll_body(ui, |ui| {
            widgets::card(ui, |ui| {
                if entries.is_empty() {
                    let text = if self.log.is_empty() { t!("history.empty") } else { t!("history.no_changes") };
                    widgets::muted(ui, text);
                }
                let footer = 28.0;
                if !entries.is_empty() {
                    egui::ScrollArea::vertical()
                    .id_salt("history_scroll")
                    .auto_shrink([false, true])
                    .max_height((ui.available_height() - footer).max(120.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (idx, entry) in entries.iter().enumerate() {
                            match entry {
                                Entry::Day(d) => {
                                    if idx > 0 {
                                        ui.add_space(12.0);
                                    }
                                    ui.label(widgets::semibold_text(day_label(*d), 13.0, MUTED));
                                    ui.add_space(6.0);
                                }
                                Entry::Line { newest, oldest, text, level, repeat, resolved } => {
                                    self.history_row(ui, (*oldest, *newest), text, *level, *repeat, *resolved);
                                }
                            }
                        }
                    });
                }
                if routine > 0 {
                    ui.add_space(8.0);
                    let mut toggle = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let tip = t!("history.routine_tip");
                        let (key, link) = if self.show_detail { ("history.routine_shown", t!("history.hide")) } else { ("history.routine_hidden", t!("history.show")) };
                        let text = format!("{} ·", i18n::fill(i18n::plural(key, routine as u64), &[("n", routine.to_string())]));
                        ui.label(RichText::new(text).size(12.5).color(SUBTLE)).on_hover_text(tip);
                        toggle = ui
                            .add(egui::Button::new(RichText::new(link).size(12.5).color(ACCENT)).frame(false))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .clicked();
                    });
                    if toggle {
                        self.show_detail = !self.show_detail;
                    }
                }
            });
        });
    }

    fn history_row(&self, ui: &mut egui::Ui, (oldest, newest): (usize, usize), text: &str, level: Level, repeat: usize, resolved: bool) {
        const LINE: f32 = 20.0;
        const TIME_W: f32 = 84.0;
        let line = &self.log[newest];
        let (text_color, family) = match level {
            Level::Success | Level::Warn => (TEXT, crate::theme::semibold()),
            Level::Error if resolved => (MUTED, FontFamily::Proportional),
            Level::Error | Level::Info => (TEXT, FontFamily::Proportional),
            Level::Detail => (SUBTLE, crate::theme::mono_soft_family()),
        };
        let head_color = if resolved { MUTED } else { DANGER };
        let first = self.log[oldest].time.format("%H:%M").to_string();
        let last = line.time.format("%H:%M").to_string();
        let time = if repeat > 1 && first != last { format!("{first}–{last}") } else { last };

        let size = if level == Level::Detail { 13.0 } else { 14.0 };
        let more = line.text.lines().count() > 1;
        let body = format!("{text}{}", if more { " …" } else { "" });
        let (head, tail) = match (level, body.split_once(": ")) {
            (Level::Error, Some((h, t))) => (format!("{h}: "), t.to_string()),
            _ => (String::new(), body),
        };
        let mut job = LayoutJob::default();
        if !head.is_empty() {
            job.append(&head, 0.0, TextFormat { font_id: FontId::new(size, crate::theme::semibold()), color: head_color, ..Default::default() });
        }
        job.append(&tail, 0.0, TextFormat { font_id: FontId::new(size, family), color: text_color, ..Default::default() });
        if repeat > 1 {
            job.append(&format!("×{repeat}"), 8.0, TextFormat { font_id: crate::theme::mono_soft(13.0), color: SUBTLE, ..Default::default() });
        }
        if resolved {
            job.append(&format!("· {}", t!("history.resolved")), 8.0, TextFormat { font_id: FontId::new(13.0, crate::theme::semibold()), color: SUCCESS, ..Default::default() });
        }
        let mut time_job = LayoutJob::default();
        time_job.append(&time, 0.0, TextFormat { font_id: crate::theme::mono_soft(13.0), color: SUBTLE, ..Default::default() });
        time_job.append(" ", 0.0, TextFormat { font_id: FontId::new(size, FontFamily::Proportional), color: SUBTLE, ..Default::default() });

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, LINE), Sense::hover());
            match level {
                Level::Success => widgets::paint_mark(ui, r.center(), Mark::Success),
                Level::Error if resolved => widgets::paint_mark(ui, r.center(), Mark::Quiet),
                Level::Error => widgets::paint_mark(ui, r.center(), Mark::Error),
                Level::Warn => {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icon::WARNING.to_string(), FontId::new(13.0, crate::theme::icons()), WARN);
                }
                Level::Info => widgets::paint_mark(ui, r.center(), Mark::Quiet),
                Level::Detail => {
                    ui.painter().circle_filled(r.center(), 2.0, CONTROL_BORDER);
                }
            }
            ui.allocate_ui_with_layout(egui::vec2(TIME_W, LINE), Layout::left_to_right(Align::Min), |ui| {
                ui.set_width(TIME_W);
                ui.add(egui::Label::new(time_job).selectable(false));
            });
            let label = egui::Label::new(job).selectable(false);
            let resp = if level == Level::Error { ui.add(label.wrap()) } else { ui.add(label.truncate()) };
            if more || level == Level::Error {
                resp.on_hover_text(&line.text);
            }
        });
        ui.add_space(8.0);
    }
}
