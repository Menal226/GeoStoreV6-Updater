use crate::app::{Action, App, Confirm};
use crate::fmt;
use crate::i18n;
use crate::logic::{PackageRow, Status};
use crate::t;
use crate::theme::*;
use crate::widgets::{self, icon, Btn, Mark, Tone};
use crate::worker::StageKind;
use chrono::Local;
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke};

const TILE: f32 = 40.0;
const TILE_GAP: f32 = 12.0;
const MORE_SLOT: f32 = 32.0;
const MIN_GAP: f32 = 16.0;
const STATE_ICON_SLOT: f32 = 18.0;

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
    version: Option<egui::text::LayoutJob>,
    portal: egui::text::LayoutJob,
    newer: bool,
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

/// Portal date; when newer than the installed one, bold with an up arrow so the state does not rest on colour alone.
fn newer_job(date: &str, newer: bool) -> egui::text::LayoutJob {
    use egui::text::TextFormat;
    let family = if newer { semibold() } else { egui::FontFamily::Proportional };
    let mut job = egui::text::LayoutJob::single_section(date.to_string(), TextFormat { font_id: egui::FontId::new(14.0, family), color: TEXT, ..Default::default() });
    if newer {
        job.append(&icon::UP.to_string(), 6.0, TextFormat { font_id: egui::FontId::new(12.0, icons()), color: ACCENT, valign: Align::Center, ..Default::default() });
    }
    job
}

fn header_text(text: &str) -> RichText {
    RichText::new(text).size(13.5).color(SUBTLE)
}

const INSTALL_LABEL: (Option<char>, &str) = (Some(icon::ADD), "dashboard.install");
const UPDATE_LABEL: (Option<char>, &str) = (Some(icon::DOWNLOAD), "dashboard.update");

/// Update and install buttons share one width so their edges line up down the column.
fn row_button_w(ui: &egui::Ui) -> f32 {
    widgets::button_width(ui, UPDATE_LABEL.0, i18n::get(UPDATE_LABEL.1)).max(widgets::button_width(ui, INSTALL_LABEL.0, i18n::get(INSTALL_LABEL.1)))
}

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
            let text = if op.update { t!("dashboard.updating") } else { t!("dashboard.installing") };
            return (text.into(), Mark::Pending);
        }
        if self.checking {
            return (t!("dashboard.checking_portal").into(), Mark::Busy);
        }
        if self.check_error.is_some() && self.rows.is_empty() {
            return (t!("dashboard.check_failed").into(), Mark::Error);
        }
        let Some(t) = self.last_check else {
            return (t!("dashboard.never_checked").into(), Mark::Empty);
        };
        let mut when = t!("dashboard.last_check", when = fmt::datetime(t));
        if let Some(next) = self.next_check() {
            when = format!("{when} · {}", t!("dashboard.next_check_short", when = fmt::datetime(next)));
        }
        if self.editor_missing() {
            (t!("dashboard.editor_missing", when = when), Mark::Pending)
        } else if pending > 0 {
            (i18n::fill(i18n::plural("dashboard.updates_available", pending as u64), &[("n", pending.to_string()), ("when", when)]), Mark::Pending)
        } else {
            (t!("dashboard.all_up_to_date", when = when), Mark::Success)
        }
    }

    fn editor_missing(&self) -> bool {
        self.rows.iter().any(|r| r.is_editor() && !r.detected)
    }

    fn row_state(&self, row: &PackageRow) -> Option<RowState> {
        let op = self.op.as_ref()?;
        if !op.keys.contains(&row.key) {
            return None;
        }
        if op.done.contains(&row.info.name) {
            Some(RowState::Finished)
        } else if let Some(p) = op.progress.as_ref().filter(|p| p.package == row.info.name) {
            let label = match p.kind {
                StageKind::Download => t!("dashboard.stage_download").into(),
                StageKind::Extract => t!("dashboard.stage_extract").into(),
                StageKind::Other => t!("dashboard.stage_install").into(),
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
        let editor_missing = self.editor_missing();
        widgets::page_header(ui, t!("dashboard.title"), &subtitle, Some(mark), |ui| {
            if editor_missing && !busy && widgets::button(ui, Some(icon::PACKAGE), t!("dashboard.new_installation"), Btn::Primary, true).clicked() {
                self.actions.push(Action::Navigate(crate::app::View::Wizard));
            }
            if !pending.is_empty()
                && widgets::button(ui, Some(icon::DOWNLOAD), &t!("dashboard.update_all", count = pending.len()), Btn::Primary, can_install).clicked()
            {
                self.actions.push(Action::Ask(Confirm::UpdateAll(pending.clone())));
            }
            let label = if checking { t!("dashboard.checking") } else { t!("dashboard.check_now") };
            let can_check = !checking && !self.bg_busy && !busy;
            let f5 = ui.input(|i| i.key_pressed(egui::Key::F5));
            if widgets::button(ui, Some(icon::REFRESH), label, Btn::Secondary, can_check).clicked() || (f5 && can_check) {
                self.actions.push(Action::Check);
            }
        });

        widgets::scroll_page(ui, "dashboard_scroll", |ui| {
            {
                self.ui_operation(ui);
                self.ui_result(ui);
                if self.bg_busy {
                    widgets::banner(
                        ui,
                        Tone::Info,
                        t!("dashboard.bg_install_title"),
                        t!("dashboard.bg_install_text"),
                        0.0,
                        |_| {},
                    );
                }

                if let Some(err) = self.check_error.clone() {
                    widgets::banner(ui, Tone::Danger, t!("dashboard.check_error"), &err, 0.0, |_| {});
                }
                if self.geostore_running && (!pending.is_empty() || self.busy()) {
                    widgets::banner(
                        ui,
                        Tone::Info,
                        t!("dashboard.editor_running_title"),
                        t!("dashboard.editor_running_text"),
                        0.0,
                        |_| {},
                    );
                }

                if self.rows.is_empty() {
                    if self.checking {
                        widgets::card(ui, |ui| {
                            ui.horizontal(|ui| {
                                widgets::spinner(ui, 16.0);
                                ui.label(RichText::new(t!("dashboard.loading_packages")).color(MUTED));
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
                        widgets::muted(ui, t!("dashboard.none_tracked"));
                    });
                } else {
                    self.ui_package_list(ui, &tracked, true, &cols);
                }

                if !others.is_empty() {
                    ui.add_space(4.0);
                    self.ui_package_list(ui, &others, false, &cols);
                }
                ui.add_space(16.0);
            }
        });
    }

    fn next_check(&self) -> Option<chrono::DateTime<Local>> {
        if !self.cfg.auto_check || !self.creds_stored || self.op.is_some() || self.bg_busy {
            return None;
        }
        Some(self.cfg.next_check(self.last_auto_check)).filter(|n| *n > Local::now())
    }

    fn row_cells(&self, row: &PackageRow, tracked: bool) -> RowCells {
        let row_state = self.row_state(row);
        let finished = matches!(row_state, Some(RowState::Finished));
        let version = row.local_version.clone().filter(|_| row.detected && !finished);
        let date = match (finished, row.detected) {
            (true, _) => row.latest.as_ref().map(|f| f.date),
            (false, false) => None,
            (false, true) => row.installed_date,
        };
        let installed = match date {
            Some(d) => RichText::new(fmt::date(d)).size(14.0).color(TEXT),
            None => RichText::new(if tracked { "—" } else { "" }).size(14.0).color(SUBTLE),
        };
        let version = match (&version, row.detected || finished) {
            (Some(v), _) => Some(widgets::numbers_job(&t!("dashboard.version", version = v), 13.5, MUTED)),
            (None, true) if finished => Some(widgets::numbers_job(t!("dashboard.version_pending"), 13.5, MUTED)),
            (None, true) => Some(widgets::numbers_job(t!("dashboard.version_unknown"), 13.5, MUTED)),
            (None, false) => None,
        };
        let newer = row.status == Status::UpdateAvailable && !finished;
        let portal_date = row.latest.as_ref().map(|f| fmt::date(f.date)).unwrap_or_else(|| "—".into());
        let state = match row_state {
            Some(RowState::Installing(label)) => StateCell::Busy(label),
            Some(RowState::Queued) => StateCell::Text(t!("dashboard.queued"), SUBTLE, None),
            Some(RowState::Finished) => {
                let label = if self.op.as_ref().is_some_and(|o| o.update) { t!("dashboard.updated") } else { t!("dashboard.installed") };
                StateCell::Text(label, SUCCESS, None)
            }
            None => match row.status {
                Status::UpdateAvailable if tracked || row.detected => StateCell::Update,
                Status::NotInstalled if !row.is_editor() && row.latest.is_some() => StateCell::Install,
                _ => {
                    let (text, color, tip) = App::status_text(row);
                    StateCell::Text(text, color, tip)
                }
            },
        };
        RowCells {
            name: widgets::semibold_text(&row.info.name, 15.0, TEXT),
            description: RichText::new(&row.info.description).size(13.5).color(MUTED),
            installed,
            version,
            portal: newer_job(&portal_date, newer),
            newer,
            state,
        }
    }

    fn measure_cols(&self, ui: &egui::Ui) -> Cols {
        let mut c = Cols {
            name: (text_width(ui, header_text(t!("dashboard.col_package"))) - TILE - TILE_GAP).max(0.0),
            installed: text_width(ui, header_text(t!("dashboard.col_installed"))),
            portal: text_width(ui, header_text(t!("dashboard.col_portal"))),
            state: text_width(ui, header_text(t!("dashboard.col_status"))),
            more_ink: text_width(ui, widgets::icon_text(icon::MORE, 14.0, MUTED)),
        };
        // Every state a row can pass through during an operation, so the grid doesn't shift when one starts.
        let busy = [t!("dashboard.stage_download"), t!("dashboard.stage_extract"), t!("dashboard.stage_install")];
        for label in busy {
            c.state = c.state.max(STATE_ICON_SLOT + text_width(ui, RichText::new(label).size(14.0)));
        }
        for text in [t!("dashboard.queued"), t!("dashboard.updated"), t!("dashboard.installed")] {
            c.state = c.state.max(STATE_ICON_SLOT + text_width(ui, widgets::semibold_text(text, 14.0, TEXT)));
        }
        c.state = c.state.max(widgets::button_width(ui, UPDATE_LABEL.0, i18n::get(UPDATE_LABEL.1)));
        for row in &self.rows {
            let cells = self.row_cells(row, self.cfg.is_tracked(&row.key, row.detected));
            c.name = c.name.max(text_width(ui, cells.name)).max(text_width(ui, cells.description));
            c.installed = c.installed.max(text_width(ui, cells.installed));
            if let Some(v) = cells.version {
                c.installed = c.installed.max(text_width(ui, v));
            }
            c.portal = c.portal.max(text_width(ui, newer_job(&row.latest.as_ref().map(|f| fmt::date(f.date)).unwrap_or_default(), true)));
            for v in [t!("dashboard.version_pending"), t!("dashboard.version_unknown")] {
                c.installed = c.installed.max(text_width(ui, widgets::numbers_job(v, 13.5, MUTED)));
            }
            let state = match &cells.state {
                StateCell::Busy(label) => STATE_ICON_SLOT + text_width(ui, RichText::new(label).size(14.0)),
                StateCell::Text(text, color, _) => STATE_ICON_SLOT + text_width(ui, widgets::semibold_text(*text, 14.0, *color)),
                StateCell::Update => widgets::button_width(ui, UPDATE_LABEL.0, i18n::get(UPDATE_LABEL.1)),
                StateCell::Install => widgets::button_width(ui, INSTALL_LABEL.0, i18n::get(INSTALL_LABEL.1)),
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

                if tracked {
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 18.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let header = |ui: &mut egui::Ui, w: f32, text: &str| {
                            ui.allocate_ui_with_layout(egui::vec2(w, 18.0), Layout::left_to_right(Align::Center), |ui| {
                                ui.set_width(w);
                                ui.label(header_text(text));
                            });
                        };
                        header(ui, TILE + TILE_GAP + name_w, t!("dashboard.col_package"));
                        ui.add_space(gap);
                        let any_installed = rows.iter().any(|r| r.detected);
                        header(ui, cols.installed, if any_installed { t!("dashboard.col_installed") } else { "" });
                        ui.add_space(gap);
                        header(ui, cols.portal, t!("dashboard.col_portal"));
                        ui.add_space(gap);
                        header(ui, cols.state, "");
                    });
                    ui.add_space(12.0);
                } else {
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 18.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let lead = TILE + TILE_GAP + name_w + gap + cols.installed + gap;
                        ui.allocate_ui_with_layout(egui::vec2(lead, 18.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(lead);
                            ui.label(widgets::semibold_text(t!("dashboard.other_packages"), 14.0, TEXT));
                        });
                        ui.label(header_text(t!("dashboard.col_portal")));
                    });
                    ui.add_space(12.0);
                }

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
        let editor_missing = self.editor_missing();
        // Registered before the row's widgets so its buttons stay on top; a right-click anywhere else opens the row menu.
        let row_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), TILE));
        let open_menu = ui.interact(row_rect, ui.id().with(("row_ctx", &row.key)), egui::Sense::click()).secondary_clicked();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            widgets::module_tile(ui, &row.info.initials, TILE);
            ui.add_space(TILE_GAP);
            cell(ui, name_w, cells.name, Some(cells.description.into()));
            ui.add_space(gap);
            cell(ui, cols.installed, cells.installed, cells.version.map(Into::into));
            ui.add_space(gap);
            let portal = cell(ui, cols.portal, cells.portal, None);
            if cells.newer {
                let name = format!("{}, {}", row.latest.as_ref().map(|f| fmt::date(f.date)).unwrap_or_default(), t!("dashboard.newer_tip"));
                portal.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &name));
            }
            ui.add_space(gap);
            ui.allocate_ui_with_layout(egui::vec2(cols.state, TILE), Layout::left_to_right(Align::Center), |ui| {
                ui.set_width(cols.state);
                match cells.state {
                    StateCell::Busy(label) => {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(widgets::icon_text(icon::DOWNLOAD, 12.0, ACCENT));
                        ui.label(RichText::new(label).size(14.0).color(ACCENT));
                    }
                    StateCell::Text(text, color, tip) => {
                        let glyph = if color == SUCCESS {
                            Some(icon::CHECK)
                        } else if color == ACCENT {
                            Some(icon::UP)
                        } else {
                            None
                        };
                        let r = ui
                            .horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if let Some(g) = glyph {
                                    ui.label(widgets::icon_text(g, 12.0, color));
                                }
                                ui.label(RichText::new(text).size(14.0).color(color));
                            })
                            .response;
                        if let Some(tip) = tip {
                            r.on_hover_text(tip);
                        }
                    }
                    StateCell::Update => {
                        let resp = widgets::button_sized(ui, UPDATE_LABEL.0, i18n::get(UPDATE_LABEL.1), Btn::Secondary, can_install, row_button_w(ui));
                        accessible_name(&resp, can_install, i18n::get(UPDATE_LABEL.1), &row.info.name, can_install);
                        if resp.clicked() {
                            self.actions.push(Action::Install(vec![row.key.clone()], false));
                        }
                    }
                    StateCell::Install if editor_missing => {
                        let resp = widgets::button_sized(ui, INSTALL_LABEL.0, i18n::get(INSTALL_LABEL.1), Btn::Secondary, true, row_button_w(ui))
                            .on_hover_text(t!("dashboard.install_needs_editor"));
                        accessible_name(&resp, true, i18n::get(INSTALL_LABEL.1), &row.info.name, true);
                        if resp.clicked() {
                            self.sync_wizard();
                            self.wizard.editor = true;
                            self.wizard.modules.insert(row.key.clone(), true);
                            self.actions.push(Action::Navigate(crate::app::View::Wizard));
                        }
                    }
                    StateCell::Install => {
                        let resp = widgets::button_sized(ui, INSTALL_LABEL.0, i18n::get(INSTALL_LABEL.1), Btn::Secondary, can_install, row_button_w(ui));
                        accessible_name(&resp, can_install, i18n::get(INSTALL_LABEL.1), &row.info.name, can_install);
                        if resp.clicked() {
                            self.actions.push(Action::Ask(Confirm::InstallModule(row.key.clone())));
                        }
                    }
                }
            });
            ui.add_space(gap - cols.more_slack());
            ui.allocate_ui_with_layout(egui::vec2(MORE_SLOT, TILE), Layout::right_to_left(Align::Center), |ui| {
                let key = row.key.clone();
                let menu_name = format!("{} – {}", t!("widgets.more_actions"), row.info.name);
                widgets::more_menu(ui, ("row_menu", &key), &menu_name, !op_running, open_menu, |ui| {
                    if row.detected && row.latest.is_some() && widgets::menu_item(ui, Some(icon::REFRESH), t!("dashboard.reinstall"), can_install).clicked() {
                        self.actions.push(Action::Ask(Confirm::Reinstall(key.clone())));
                    }
                    if row.status == Status::UpdateAvailable && widgets::menu_item(ui, Some(icon::CHECK), t!("dashboard.mark_current"), true).clicked() {
                        self.actions.push(Action::MarkCurrent(key.clone()));
                    }
                    let (ic, label) = if tracked { (icon::HIDE, t!("dashboard.stop_tracking")) } else { (icon::VIEW, t!("dashboard.track")) };
                    if widgets::menu_item(ui, Some(ic), label, true).clicked() {
                        self.actions.push(Action::SetTracked(key.clone(), !tracked));
                    }
                });
            });
        });
    }
}

/// A row button's screen-reader name: the action, the package, and why it is unavailable while an installation runs.
fn accessible_name(resp: &egui::Response, enabled: bool, action: &str, package: &str, can_install: bool) {
    let mut name = format!("{action} – {package}");
    if !can_install {
        name = format!("{name} ({})", t!("dashboard.wait_for_op"));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
}
