use crate::app::{Action, App, Confirm};
use crate::fmt;
use crate::logic::{PackageRow, Status};
use crate::theme::*;
use crate::widgets::{self, icon, Btn, Mark, Tone};
use chrono::Local;
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke};
use std::time::Instant;

const TILE: f32 = 40.0;
const TILE_GAP: f32 = 12.0;
const MORE_SLOT: f32 = 32.0;
const MIN_GAP: f32 = 16.0;

fn updates_word(n: usize) -> &'static str {
    if n >= 5 { "aktualizací" } else { "aktualizace" }
}

enum RowState {
    Installing(String),
    Queued,
    Finished,
}

enum StateCell {
    Busy(String),
    Text(&'static str, egui::Color32, Option<&'static str>),
    Update,
    Install,
}

struct RowCells {
    name: RichText,
    description: RichText,
    installed: RichText,
    version: egui::text::LayoutJob,
    installed_tip: &'static str,
    portal: RichText,
    state: StateCell,
}

/// Widest content of each column across every row on the page, so both tables share one grid.
struct Cols {
    name: f32,
    installed: f32,
    portal: f32,
    state: f32,
    more_ink: f32,
}

impl Cols {
    fn more_slack(&self) -> f32 {
        ((MORE_SLOT - self.more_ink) / 2.0).max(0.0)
    }

    /// Equal gap between the widest boxes of neighbouring columns, and the name width that fits with it.
    fn fit(&self, total: f32) -> (f32, f32) {
        let fixed = TILE + TILE_GAP + self.installed + self.portal + self.state + self.more_ink + self.more_slack();
        let gap = ((total - fixed - self.name) / 4.0).max(MIN_GAP);
        (gap, (total - fixed - gap * 4.0).max(80.0))
    }
}

fn text_width(ui: &egui::Ui, text: impl Into<egui::WidgetText>) -> f32 {
    text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body).size().x
}

fn header_text(text: &str) -> RichText {
    RichText::new(text).size(12.0).color(SUBTLE)
}

const INSTALL_LABEL: (Option<char>, &str) = (None, "Nainstalovat");
const UPDATE_LABEL: (Option<char>, &str) = (Some(icon::DOWNLOAD), "Aktualizovat");

/// One or two text lines, centred as a block within the row height.
fn cell(ui: &mut egui::Ui, width: f32, first: impl Into<egui::WidgetText>, second: Option<egui::WidgetText>) -> egui::Response {
    const LINE_GAP: f32 = 1.0;
    let layout = |t: egui::WidgetText| t.into_galley(ui, Some(egui::TextWrapMode::Truncate), width, egui::TextStyle::Body);
    let first = layout(first.into());
    let second = second.map(layout);
    let height = first.size().y + second.as_ref().map_or(0.0, |g| LINE_GAP + g.size().y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, TILE), egui::Sense::hover());
    let mut y = rect.center().y - height / 2.0;
    let next = first.size().y + LINE_GAP;
    ui.painter().galley(egui::pos2(rect.left(), y), first, TEXT);
    y += next;
    if let Some(g) = second {
        ui.painter().galley(egui::pos2(rect.left(), y), g, TEXT);
    }
    resp
}

impl App {
    fn dashboard_status(&self, pending: usize) -> (String, Mark) {
        if let Some(op) = &self.op {
            let text = match op.progress.as_ref().filter(|p| p.steps > 0) {
                Some(p) if p.steps > 1 => format!("Probíhá instalace · balíček {} ze {}", p.step, p.steps),
                _ => "Probíhá instalace".into(),
            };
            return (text, Mark::Busy);
        }
        if self.checking {
            return ("Kontroluji dostupné balíčky na portálu…".into(), Mark::Busy);
        }
        if self.check_error.is_some() && self.rows.is_empty() {
            return ("Kontrola se nezdařila".into(), Mark::Error);
        }
        let Some(t) = self.last_check else {
            return ("Zatím neproběhla žádná kontrola".into(), Mark::Empty);
        };
        let when = format!("poslední kontrola {}", fmt::datetime(t));
        if pending > 0 {
            (format!("K dispozici {pending} {} · {when}", updates_word(pending)), Mark::Error)
        } else {
            (format!("Vše aktuální · {when}"), Mark::Success)
        }
    }

    fn row_state(&self, row: &PackageRow) -> Option<RowState> {
        let op = self.op.as_ref()?;
        if !op.keys.contains(&row.key) {
            return None;
        }
        if op.done.contains(&row.info.name) {
            Some(RowState::Finished)
        } else if let Some(p) = op.progress.as_ref().filter(|p| p.package == row.info.name) {
            let label = if p.stage.starts_with("Stahování") {
                match p.fraction {
                    Some(f) => format!("Stahování · {:.0} %", f * 100.0),
                    None => "Stahování…".into(),
                }
            } else if p.stage.starts_with("Rozbalování") {
                "Rozbalování…".into()
            } else {
                "Instalace…".into()
            };
            Some(RowState::Installing(label))
        } else {
            Some(RowState::Queued)
        }
    }

    pub(crate) fn ui_dashboard(&mut self, ui: &mut egui::Ui) {
        let pending: Vec<String> = self
            .pending_keys()
            .into_iter()
            .filter(|k| self.row(k).is_none_or(|r| !self.finished_in_op(r)))
            .collect();
        let (subtitle, mark) = self.dashboard_status(pending.len());
        let can_install = self.can_install();
        let checking = self.checking;
        let busy = self.busy();
        widgets::page_header(ui, "Přehled", &subtitle, Some(mark), |ui| {
            if !pending.is_empty()
                && !busy
                && widgets::button(ui, Some(icon::DOWNLOAD), &format!("Aktualizovat vše ({})", pending.len()), Btn::Primary, can_install).clicked()
            {
                self.actions.push(Action::Ask(Confirm::UpdateAll(pending.clone())));
            }
            let label = if checking { "Kontroluji…" } else { "Zkontrolovat" };
            if !busy && widgets::button(ui, Some(icon::REFRESH), label, Btn::Secondary, !checking).clicked() {
                self.actions.push(Action::Check);
            }
        });

        widgets::scroll_page(ui, "dashboard_scroll", |ui| {
            {
                self.ui_operation(ui);
                self.ui_result(ui);
                self.ui_countdown(ui);

                if let Some(err) = self.check_error.clone() {
                    widgets::banner(ui, Tone::Danger, "Kontrola aktualizací selhala", &err, 0.0, |_| {});
                }
                if self.geostore_running && (!pending.is_empty() || self.busy()) {
                    widgets::banner(
                        ui,
                        Tone::Info,
                        "GeoStore V6 je spuštěn",
                        "Před instalací aktualizací jej prosím zavřete. Spuštěná instalace na jeho ukončení počká.",
                        0.0,
                        |_| {},
                    );
                }

                if self.rows.is_empty() {
                    if self.checking {
                        widgets::card(ui, |ui| {
                            ui.horizontal(|ui| {
                                widgets::spinner(ui, 16.0);
                                ui.label(RichText::new("Načítám seznam balíčků z portálu…").color(MUTED));
                            });
                        });
                    }
                    return;
                }

                let cols = self.measure_cols(ui);
                let rows = self.rows.clone();
                let (tracked, others): (Vec<PackageRow>, Vec<PackageRow>) =
                    rows.into_iter().partition(|r| self.cfg.is_tracked(&r.key, r.detected));

                if tracked.is_empty() {
                    widgets::card(ui, |ui| {
                        widgets::muted(ui, "Žádný balíček není sledován. Nainstalujte řešení v sekci Nová instalace nebo zapněte sledování níže.");
                    });
                } else {
                    self.ui_package_list(ui, &tracked, true, &cols);
                }
                self.ui_next_check(ui);

                if !others.is_empty() {
                    widgets::section_title(
                        ui,
                        "Další balíčky dostupné pro váš účet",
                        "Nejsou sledovány. Sledování zapnete v nabídce u balíčku nebo v Nastavení",
                    );
                    self.ui_package_list(ui, &others, false, &cols);
                }
                ui.add_space(16.0);
            }
        });
    }

    fn ui_next_check(&self, ui: &mut egui::Ui) {
        if !self.cfg.auto_check || self.op.is_some() {
            return;
        }
        let Some(last) = self.last_check else { return };
        let next = last + chrono::Duration::hours(self.cfg.check_interval_hours.max(1) as i64);
        let text = if next <= Local::now() {
            "Další automatická kontrola proběhne za chvíli".to_string()
        } else {
            format!("Další automatická kontrola {}", fmt::datetime(next))
        };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(widgets::icon_text(icon::REFRESH, 12.0, SUBTLE));
            ui.label(RichText::new(text).size(12.5).color(SUBTLE));
        });
    }

    fn ui_countdown(&mut self, ui: &mut egui::Ui) {
        let Some(at) = self.countdown else { return };
        let secs = at.saturating_duration_since(Instant::now()).as_secs() + 1;
        let text = format!("Instalace začne za {secs} s. Automatickou instalaci vypnete v Nastavení");
        widgets::banner(ui, Tone::Info, "Aktualizace se nainstalují automaticky", &text, 248.0, |ui| {
            if widgets::button(ui, Some(icon::PLAY), "Instalovat hned", Btn::Primary, true).clicked() {
                self.actions.push(Action::InstallNow);
            }
            if widgets::button(ui, None, "Nyní ne", Btn::Secondary, true).clicked() {
                self.actions.push(Action::DismissCountdown);
            }
        });
    }

    fn row_cells(&self, row: &PackageRow, tracked: bool) -> RowCells {
        let row_state = self.row_state(row);
        let finished = matches!(row_state, Some(RowState::Finished));
        let version = row.local_version.clone().filter(|_| row.detected || finished);
        let (date, installed_tip) = match (finished, row.detected, row.installed_date) {
            (true, _, _) => (row.latest.as_ref().map(|f| f.date), "Balíček byl právě nainstalován touto aplikací"),
            (false, false, _) => (None, ""),
            (false, true, d) if row.from_state => (d, "Datum vydání balíčku, který tato aplikace nainstalovala"),
            (false, true, d) => (d, "Datum nainstalovaných souborů. Balíček nebyl instalován touto aplikací, stav se proto odhaduje."),
        };
        let installed = match date {
            Some(d) => widgets::mono_text(fmt::date(d), 13.0, if finished { SUCCESS } else { MONO_INK }),
            None => widgets::mono_text("—", 13.0, SUBTLE),
        };
        let version = match (&version, row.detected || finished) {
            (Some(v), _) => widgets::numbers_job(&format!("verze {v}"), 12.5, SECONDARY_INK),
            (None, true) => widgets::numbers_job("verze neznámá", 12.5, SECONDARY_INK),
            (None, false) => widgets::numbers_job("nenainstalováno", 12.5, SECONDARY_INK),
        };
        let newer = row.status == Status::UpdateAvailable && !finished;
        let portal_date = row.latest.as_ref().map(|f| fmt::date(f.date)).unwrap_or_else(|| "—".into());
        let op_running = self.op.is_some();
        let state = match row_state {
            Some(RowState::Installing(label)) => StateCell::Busy(label),
            Some(RowState::Queued) => StateCell::Text("Čeká ve frontě", SUBTLE, None),
            Some(RowState::Finished) => {
                let label = if self.op.as_ref().is_some_and(|o| o.title == "Instalace aktualizací") { "Aktualizováno" } else { "Nainstalováno" };
                StateCell::Text(label, SUCCESS, None)
            }
            None => match row.status {
                Status::UpdateAvailable if (tracked || row.detected) && !op_running => StateCell::Update,
                Status::NotInstalled if !row.is_editor() && row.latest.is_some() && !op_running => StateCell::Install,
                _ => {
                    let (text, color, tip) = App::status_text(row);
                    StateCell::Text(text, color, tip)
                }
            },
        };
        RowCells {
            name: widgets::semibold_text(&row.info.name, 15.0, if tracked { TEXT } else { MUTED }),
            description: RichText::new(&row.info.description).size(13.0).color(MUTED),
            installed,
            version,
            installed_tip,
            portal: widgets::mono_text(portal_date, 13.0, if newer { ACCENT } else { MONO_INK }),
            state,
        }
    }

    fn measure_cols(&self, ui: &egui::Ui) -> Cols {
        let mut c = Cols {
            name: (text_width(ui, header_text("Balíček")) - TILE - TILE_GAP).max(0.0),
            installed: text_width(ui, header_text("Nainstalováno")),
            portal: text_width(ui, header_text("Na portálu")),
            state: text_width(ui, header_text("Stav")),
            more_ink: text_width(ui, widgets::icon_text(icon::MORE, 14.0, MUTED)),
        };
        for row in &self.rows {
            let cells = self.row_cells(row, self.cfg.is_tracked(&row.key, row.detected));
            c.name = c.name.max(text_width(ui, cells.name)).max(text_width(ui, cells.description));
            c.installed = c.installed.max(text_width(ui, cells.installed)).max(text_width(ui, cells.version));
            c.portal = c.portal.max(text_width(ui, cells.portal));
            let state = match &cells.state {
                StateCell::Busy(label) => widgets::MARK_SLOT + text_width(ui, RichText::new(label).size(13.0)),
                StateCell::Text(text, color, _) => text_width(ui, widgets::semibold_text(*text, 13.0, *color)),
                StateCell::Update => widgets::button_width(ui, UPDATE_LABEL.0, UPDATE_LABEL.1),
                StateCell::Install => widgets::button_width(ui, INSTALL_LABEL.0, INSTALL_LABEL.1),
            };
            c.state = c.state.max(state);
        }
        c
    }

    fn ui_package_list(&mut self, ui: &mut egui::Ui, rows: &[PackageRow], tracked: bool, cols: &Cols) {
        Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(20, 0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                let (gap, name_w) = cols.fit(ui.available_width());

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let header = |ui: &mut egui::Ui, w: f32, text: &str, tip: Option<&str>| {
                        ui.allocate_ui_with_layout(egui::vec2(w, 16.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(w);
                            let r = ui.label(header_text(text));
                            if let Some(t) = tip {
                                r.on_hover_text(t);
                            }
                        });
                    };
                    header(ui, TILE + TILE_GAP + name_w, "Balíček", None);
                    ui.add_space(gap);
                    header(ui, cols.installed, "Nainstalováno", Some("Datum vydání nainstalovaného balíčku a verze nainstalovaných souborů"));
                    ui.add_space(gap);
                    header(ui, cols.portal, "Na portálu", Some("Datum vydání nejnovějšího instalačního balíčku na portálu. Aktualizace se nabízí, když je novější než nainstalovaný."));
                    ui.add_space(gap);
                    header(ui, cols.state, "Stav", None);
                });
                ui.add_space(12.0);

                for row in rows {
                    let y = ui.cursor().top();
                    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0_f32, BORDER));
                    ui.add_space(12.0);
                    self.ui_package_row(ui, row, tracked, cols, gap, name_w);
                    ui.add_space(12.0);
                }
            });
        ui.add_space(8.0);
    }

    fn ui_package_row(&mut self, ui: &mut egui::Ui, row: &PackageRow, tracked: bool, cols: &Cols, gap: f32, name_w: f32) {
        let can_install = self.can_install();
        let op_running = self.op.is_some();
        let cells = self.row_cells(row, tracked);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            widgets::module_tile(ui, &row.info.initials, !tracked, TILE);
            ui.add_space(TILE_GAP);
            let name = cell(ui, name_w, cells.name, Some(cells.description.into()));
            if let Some(f) = &row.latest {
                name.on_hover_text(format!("{}\n{}", row.info.description, f.title));
            }
            ui.add_space(gap);
            let installed = cell(ui, cols.installed, cells.installed, Some(cells.version.into()));
            if !cells.installed_tip.is_empty() {
                installed.on_hover_text(cells.installed_tip);
            }
            ui.add_space(gap);
            cell(ui, cols.portal, cells.portal, None);
            ui.add_space(gap);
            ui.allocate_ui_with_layout(egui::vec2(cols.state, TILE), Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(cols.state);
                match cells.state {
                    StateCell::Busy(label) => {
                        widgets::mark_label(ui, Mark::Busy, &label, 13.0);
                    }
                    StateCell::Text(text, color, tip) => {
                        let r = ui.label(widgets::semibold_text(text, 13.0, color));
                        if let Some(tip) = tip {
                            r.on_hover_text(tip);
                        }
                    }
                    StateCell::Update => {
                        if widgets::button(ui, UPDATE_LABEL.0, UPDATE_LABEL.1, Btn::Secondary, can_install).clicked() {
                            self.actions.push(Action::Install(vec![row.key.clone()], false));
                        }
                    }
                    StateCell::Install => {
                        if widgets::button(ui, INSTALL_LABEL.0, INSTALL_LABEL.1, Btn::Secondary, can_install).clicked() {
                            self.actions.push(Action::Ask(Confirm::InstallModule(row.key.clone())));
                        }
                    }
                }
            });
            ui.add_space(gap - cols.more_slack());
            ui.allocate_ui_with_layout(egui::vec2(MORE_SLOT, TILE), Layout::right_to_left(Align::Center), |ui| {
                if op_running {
                    return;
                }
                let key = row.key.clone();
                let elevated = self.elevated;
                widgets::more_menu(ui, ("row_menu", &key), |ui| {
                    if row.detected && row.latest.is_some() {
                        let item = widgets::menu_item(ui, Some(icon::REFRESH), "Přeinstalovat", can_install);
                        if !elevated {
                            item.on_hover_text("Instalace vyžaduje spuštění aplikace s právy správce");
                        } else if item.clicked() {
                            self.actions.push(Action::Ask(Confirm::Reinstall(key.clone())));
                        }
                    }
                    if row.status == Status::UpdateAvailable && widgets::menu_item(ui, Some(icon::CHECK), "Označit jako aktuální", true).clicked() {
                        self.actions.push(Action::MarkCurrent(key.clone()));
                    }
                    let (ic, label) = if tracked { (icon::HIDE, "Přestat sledovat") } else { (icon::VIEW, "Sledovat aktualizace") };
                    if widgets::menu_item(ui, Some(ic), label, true).clicked() {
                        self.actions.push(Action::SetTracked(key.clone(), !tracked));
                    }
                });
            });
        });
    }
}
