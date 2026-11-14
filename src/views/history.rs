use crate::app::App;
use crate::fmt;
use crate::i18n;
use crate::logfile::{self, Level, LogLine};
use crate::paths;
use crate::t;
use crate::theme::*;
use crate::widgets::{self, icon, Mark};
use chrono::{Local, NaiveDate};
use eframe::egui::{self, text::LayoutJob, Align, FontFamily, FontId, Layout, RichText, Sense, TextFormat};

enum Entry {
    Day(NaiveDate),
    Line { newest: usize, oldest: usize, text: String, level: Level, repeat: usize, resolved: bool, children: Vec<String> },
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
        let full = i18n::plain(&i18n::fill(i18n::get_in(lang, "log.check_done"), &[("outcome", i18n::get_in(lang, "log.all_up_to_date").to_string())]));
        let prefix = full.split('{').next().unwrap_or_default();
        !prefix.is_empty() && text.starts_with(prefix)
    })
}

/// Current wording for a log line (older log files used different phrasing); routine lines become Detail.
fn normalize(level: Level, text: &str) -> (Level, String) {
    let mut text = i18n::plain(text.lines().next().unwrap_or_default());
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
    for old in ["Nová instalace: ", "New installation: "] {
        if let Some(rest) = text.strip_prefix(old) {
            text = format!("{}: {rest}", t!("worker.title_clean"));
        }
    }
    let text = text.trim_end_matches('.').to_string();
    let text = i18n::translate_fixed(&text).map_or(text, str::to_string);
    let routine = i18n::starts_with_any(&text, "log.logged_in") || all_up_to_date(&text);
    let level = if routine && level == Level::Info { Level::Detail } else { level };
    (level, text)
}

/// The page's rows, rebuilt only when the log, the language or the detail switch changes rather than on every frame.
pub(crate) struct HistoryCache {
    rev: u64,
    detail: bool,
    entries: Vec<Entry>,
    routine: usize,
}

struct Item {
    idx: usize,
    level: Level,
    text: String,
    children: Vec<String>,
}

/// Whether `text` was produced from the template `key` in any language: same fixed start and end, ignoring a final period.
fn from_template(text: &str, key: &'static str) -> bool {
    i18n::Lang::ALL.iter().any(|&lang| {
        let template = i18n::plain(i18n::get_in(lang, key));
        let template = template.trim_end_matches('.');
        let prefix = template.split('{').next().unwrap_or_default();
        let suffix = template.rsplit('}').next().unwrap_or_default();
        let fixed = !template.contains('{');
        (fixed && text.starts_with(template)) || (!fixed && (!prefix.is_empty() || !suffix.is_empty()) && text.starts_with(prefix) && text.ends_with(suffix))
    })
}

/// "Name – file.zip (date)" lines are dropped when the same package also has a "Name: result" line.
fn drop_file_lines(children: &mut Vec<String>) {
    let results: Vec<String> = children.iter().filter_map(|c| c.split_once(": ").map(|(n, _)| n.to_string())).collect();
    children.retain(|c| !c.split_once(" – ").is_some_and(|(n, _)| results.iter().any(|r| r == n)));
}

fn is_result(text: &str) -> bool {
    ["worker.install_ok", "worker.install_warnings", "worker.cancelled", "worker.install_failed"]
        .into_iter()
        .any(|k| from_template(text, k))
}

/// Chronological log lines, with each operation folded into its result line: the steps logged between
/// "started" and the result, and the result's own continuation lines, become that line's children.
fn fold_operations(log: &[LogLine], detail: bool) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    let mut op_start: Option<usize> = None;
    let mut open_result = false;
    for (i, l) in log.iter().enumerate() {
        let (level, text) = normalize(l.level, &l.text);
        let continuation = open_result && i > 0 && log[i - 1].time == l.time && log[i - 1].level == l.level;
        if continuation {
            if let Some(last) = items.last_mut() {
                if !from_template(&text, "worker.demo_note") {
                    last.children.push(text.trim_start_matches("• ").to_string());
                }
            }
            continue;
        }
        open_result = false;
        if from_template(&text, "worker.op_started") {
            op_start = Some(items.len());
        } else if is_result(&text) {
            let mut children = Vec::new();
            let mut head = text.clone();
            if let Some(start) = op_start.take() {
                let mut steps = items.drain(start..);
                // A successful operation is titled by its name ("Instalace aktualizací"); other outcomes keep their message as the title.
                if let Some(title) = steps.next().and_then(|s| s.text.rsplit_once(": ").map(|(t, _)| t.to_string())) {
                    if level == Level::Success {
                        head = title;
                    }
                }
                children = steps.filter(|it| detail || it.level != Level::Detail).map(|it| it.text).collect();
            }
            items.push(Item { idx: i, level, text: head, children });
            open_result = true;
            continue;
        }
        items.push(Item { idx: i, level, text, children: Vec::new() });
    }
    items
}

impl HistoryCache {
    fn build(log: &[LogLine], rev: u64, detail: bool) -> Self {
        let mut entries: Vec<Entry> = Vec::new();
        let mut current: Option<NaiveDate> = None;
        let mut later_success = false;
        let mut routine = 0;
        for Item { idx: i, level, text, mut children } in fold_operations(log, detail).into_iter().rev() {
            drop_file_lines(&mut children);
            let l = &log[i];
            let resolved = level == Level::Error && later_success;
            later_success |= l.level == Level::Success || i18n::starts_with_any(&text, "log.check_done");
            if level == Level::Detail {
                routine += 1;
                if !detail {
                    continue;
                }
            }
            let d = l.time.date_naive();
            if current != Some(d) {
                current = Some(d);
                entries.push(Entry::Day(d));
            } else if let Some(Entry::Line { text: prev, level: prev_level, repeat, oldest, children: prev_children, .. }) = entries.last_mut() {
                if *prev == text && *prev_level == level && children.is_empty() && prev_children.is_empty() {
                    *repeat += 1;
                    *oldest = i;
                    continue;
                }
            }
            entries.push(Entry::Line { newest: i, oldest: i, text, level, repeat: 1, resolved, children });
        }
        Self { rev, detail, entries, routine }
    }
}

impl App {
    pub(crate) fn ui_history(&mut self, ui: &mut egui::Ui) {
        let cache = match self.history.take() {
            Some(c) if c.rev == self.log_rev && c.detail == self.show_detail => c,
            _ => HistoryCache::build(&self.log, self.log_rev, self.show_detail),
        };
        let routine = cache.routine;
        let mut show_detail = self.show_detail;
        widgets::page_header(ui, t!("history.title"), "", None, |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            widgets::menu_button(ui, "history_menu", None, t!("history.logs"), |ui| {
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
            if routine > 0 || show_detail {
                widgets::checkbox(ui, &mut show_detail, RichText::new(t!("history.show_routine", n = routine)).size(14.0), true);
            }
        });
        if show_detail != self.show_detail {
            self.show_detail = show_detail;
        }
        let entries = &cache.entries;

        widgets::scroll_body(ui, |ui| {
            widgets::card(ui, |ui| {
                if entries.is_empty() {
                    let text = if self.log.is_empty() { t!("history.empty") } else { t!("history.no_changes") };
                    widgets::muted(ui, text);
                }
                if !entries.is_empty() {
                    egui::ScrollArea::vertical()
                    .id_salt("history_scroll")
                    .auto_shrink([false, true])
                    .max_height(ui.available_height().max(120.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (idx, entry) in entries.iter().enumerate() {
                            match entry {
                                Entry::Day(d) => {
                                    if idx > 0 {
                                        ui.add_space(12.0);
                                    }
                                    ui.label(widgets::semibold_text(day_label(*d), 13.5, MUTED));
                                    ui.add_space(10.0);
                                }
                                Entry::Line { newest, oldest, text, level, repeat, resolved, children } => {
                                    self.history_row(ui, (*oldest, *newest), text, *level, *repeat, *resolved, children, idx + 1 == entries.len());
                                }
                            }
                        }
                    });
                }
            });
        });
        self.history = Some(cache);
    }

    #[allow(clippy::too_many_arguments)]
    fn history_row(&self, ui: &mut egui::Ui, (oldest, newest): (usize, usize), text: &str, level: Level, repeat: usize, resolved: bool, children: &[String], last_row: bool) {
        const LINE: f32 = 20.0;
        const TIME_W: f32 = 48.0;
        let line = &self.log[newest];
        let (text_color, family) = match level {
            Level::Success | Level::Warn => (TEXT, crate::theme::semibold()),
            Level::Error if resolved => (MUTED, FontFamily::Proportional),
            Level::Error => (TEXT, FontFamily::Proportional),
            Level::Info => (MUTED, FontFamily::Proportional),
            Level::Detail => (SUBTLE, FontFamily::Proportional),
        };
        let head_color = if resolved { MUTED } else { DANGER };
        let first = self.log[oldest].time.format("%H:%M").to_string();
        let time = line.time.format("%H:%M").to_string();

        let size = if level == Level::Detail { 13.0 } else { 14.0 };
        let (head, tail) = match (level, text.split_once(": ")) {
            (Level::Error, Some((h, t))) => (format!("{h}: "), t.to_string()),
            _ => (String::new(), text.to_string()),
        };
        let mut job = LayoutJob::default();
        if !head.is_empty() {
            job.append(&head, 0.0, TextFormat { font_id: FontId::new(size, crate::theme::semibold()), color: head_color, ..Default::default() });
        }
        job.append(&tail, 0.0, TextFormat { font_id: FontId::new(size, family), color: text_color, ..Default::default() });
        if repeat > 1 {
            let repeat_text = if first == time { format!("{repeat}×") } else { t!("history.repeat_inline", n = repeat, from = first) };
            job.append(&format!("· {repeat_text}"), 8.0, TextFormat { font_id: FontId::new(13.5, FontFamily::Proportional), color: SUBTLE, ..Default::default() });
        }
        if resolved {
            job.append(&format!("· {}", t!("history.resolved")), 8.0, TextFormat { font_id: FontId::new(13.0, crate::theme::semibold()), color: SUCCESS, ..Default::default() });
        }

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, LINE), Sense::hover());
            let glyph = |ui: &egui::Ui, c: char, color| {
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, c.to_string(), FontId::new(14.0, crate::theme::icons()), color);
            };
            match level {
                Level::Success => glyph(ui, icon::CHECK, SUCCESS),
                Level::Error if resolved => widgets::paint_mark(ui, r.center(), Mark::Quiet),
                Level::Error => glyph(ui, icon::ERROR, DANGER),
                Level::Warn => glyph(ui, icon::WARNING, WARN),
                Level::Info => {
                    ui.painter().circle_filled(r.center() + egui::vec2(0.0, 1.5), 3.0, MUTED);
                }
                Level::Detail => {
                    ui.painter().circle_filled(r.center(), 2.0, CONTROL_BORDER);
                }
            }
            ui.allocate_ui_with_layout(egui::vec2(TIME_W, LINE), Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(TIME_W);
                ui.add(egui::Label::new(RichText::new(&time).size(13.5).color(SUBTLE)).selectable(false));
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let label = egui::Label::new(job).selectable(false);
                let resp = if level == Level::Error { ui.add(label.wrap()) } else { ui.add(label.truncate()) };
                if level == Level::Error {
                    resp.on_hover_text(&line.text);
                }
                for c in children {
                    ui.add(egui::Label::new(RichText::new(c).size(13.5).color(MUTED)).selectable(false).truncate());
                }
            });
        });
        if !last_row {
            ui.add_space(8.0);
        }
    }
}
