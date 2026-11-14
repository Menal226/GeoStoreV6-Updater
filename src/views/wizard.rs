use crate::app::{Action, App, Confirm, Licences};
use crate::i18n;
use crate::logic::{PackageRow, Status};
use crate::t;
use crate::theme::*;
use crate::widgets::{self, icon, Btn};
use crate::worker::{CleanPlan, Cmd};
use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Stroke};

/// The bar sits on the panel's 20px bottom margin; this top inset gives the button equal space above and below.
const BAR_TOP: f32 = 17.0;

impl App {
    pub(crate) fn sync_wizard(&mut self) {
        if self.wizard.initialized || self.rows.is_empty() {
            return;
        }
        let editor_installed = self.rows.iter().any(|r| r.is_editor() && r.detected);
        self.wizard.editor = !editor_installed;
        self.wizard.hasp = !editor_installed;
        self.wizard.vcredist = !editor_installed;
        self.wizard.licence = true;
        for r in self.rows.iter().filter(|r| !r.is_editor()) {
            self.wizard.modules.insert(r.key.clone(), false);
        }
        self.wizard.initialized = true;
    }

    fn wizard_plan(&self) -> CleanPlan {
        CleanPlan {
            editor: self.wizard.editor,
            modules: self
                .wizard
                .modules
                .iter()
                .filter(|(_, on)| **on)
                .map(|(k, _)| k.clone())
                .collect(),
            hasp: self.wizard.hasp,
            vcredist: self.wizard.vcredist && self.wizard.editor,
            licence: self.chosen_licence(),
        }
    }

    fn no_licences(&self) -> bool {
        matches!(&self.licences, Licences::Ready(l) if l.is_empty())
    }

    fn licence_wanted(&self) -> bool {
        self.wizard.licence && self.licence_possible() && !self.no_licences()
    }

    fn chosen_licence(&self) -> Option<String> {
        if !self.licence_wanted() {
            return None;
        }
        match &self.licences {
            Licences::Ready(list) => self.wizard.licence_guid.clone().filter(|g| list.iter().any(|l| &l.guid == g)),
            _ => None,
        }
    }

    fn licence_blocker(&self) -> Option<&'static str> {
        if !self.licence_wanted() {
            return None;
        }
        match &self.licences {
            Licences::Idle | Licences::Loading => Some(t!("wizard.licences_loading")),
            Licences::Failed(_) => Some(t!("wizard.licences_failed")),
            Licences::Ready(_) => None,
        }
    }

    fn ui_licence_choice(&mut self, ui: &mut egui::Ui, enabled: bool) {
        match &self.licences {
            Licences::Idle | Licences::Loading => {
                ui.horizontal(|ui| {
                    widgets::spinner(ui, 14.0);
                    ui.label(RichText::new(t!("wizard.licences_loading")).size(13.0).color(MUTED));
                });
            }
            Licences::Failed(e) => {
                let e = e.clone();
                ui.add(egui::Label::new(RichText::new(t!("wizard.licences_failed_detail", error = e)).size(13.0).color(WARN)).wrap());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::button(ui, Some(icon::REFRESH), t!("wizard.retry"), Btn::Secondary, true).clicked() {
                        self.licences = Licences::Idle;
                    }
                    if widgets::button(ui, Some(icon::USER), t!("wizard.licman_other_account"), Btn::Secondary, true).clicked() {
                        self.actions.push(Action::LicmanLogin);
                    }
                    if self.licman_account.is_some() && widgets::button(ui, None, t!("wizard.licman_use_portal"), Btn::Secondary, true).clicked() {
                        self.actions.push(Action::LicmanReset);
                    }
                });
            }
            Licences::Ready(list) if list.is_empty() => {
                ui.label(RichText::new(t!("wizard.no_licences")).size(13.0).color(WARN));
            }
            Licences::Ready(list) if list.len() == 1 => {
                let l = &list[0];
                ui.label(RichText::new(l.title()).size(14.0).color(if enabled { TEXT } else { DISABLED_TEXT }));
                ui.add_space(2.0);
                ui.label(RichText::new(l.detail()).size(13.0).color(if enabled { MUTED } else { DISABLED_TEXT }));
            }
            Licences::Ready(list) => {
                let list = list.clone();
                let n = list.len();
                let text = i18n::fill(i18n::plural("wizard.choose_licence", n as u64), &[("n", n.to_string())]);
                ui.label(RichText::new(text).size(13.0).color(if enabled { MUTED } else { DISABLED_TEXT }));
                for l in &list {
                    ui.add_space(8.0);
                    let selected = self.wizard.licence_guid.as_deref() == Some(l.guid.as_str());
                    if widgets::radio(ui, selected, &l.title(), &l.detail(), enabled).clicked() {
                        self.wizard.licence_guid = Some(l.guid.clone());
                    }
                }
            }
        }
    }

    fn ui_licman_account(&mut self, ui: &mut egui::Ui, enabled: bool) {
        if !matches!(self.licences, Licences::Ready(_)) {
            return;
        }
        let link = |ui: &mut egui::Ui, text: &str| {
            ui.add_enabled(enabled, egui::Button::new(RichText::new(text).size(13.0).color(if enabled { ACCENT } else { DISABLED_TEXT })).frame(false))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        };
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let color = if enabled { MUTED } else { DISABLED_TEXT };
            match self.licman_account.clone() {
                Some(user) => {
                    ui.label(RichText::new(t!("wizard.licman_account", user = user)).size(13.0).color(color));
                    if link(ui, t!("wizard.licman_change")) {
                        self.actions.push(Action::LicmanLogin);
                    }
                    if link(ui, t!("wizard.licman_use_portal")) {
                        self.actions.push(Action::LicmanReset);
                    }
                }
                None => {
                    ui.label(RichText::new(t!("wizard.licman_account", user = self.user)).size(13.0).color(color));
                    if link(ui, t!("wizard.licman_other_account")) {
                        self.actions.push(Action::LicmanLogin);
                    }
                }
            }
        });
    }

    fn licence_possible(&self) -> bool {
        self.wizard.editor || self.rows.iter().any(|r| r.is_editor() && r.detected)
    }

    fn wizard_item(ui: &mut egui::Ui, checked: &mut bool, enabled: bool, name: &str, row: &PackageRow) {
        ui.horizontal(|ui| {
            widgets::checkbox(ui, checked, widgets::semibold_text(name, 14.0, if enabled { TEXT } else { DISABLED_TEXT }), enabled);
            if row.detected {
                let (text, color) = match row.status {
                    Status::UpdateAvailable => (t!("wizard.installed_outdated"), DANGER),
                    _ => (t!("wizard.installed_current"), SUCCESS),
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(widgets::semibold_text(text, 13.0, color));
                });
            }
        });
        ui.add_space(-6.0);
    }

    pub(crate) fn ui_wizard(&mut self, ui: &mut egui::Ui) {
        self.sync_wizard();
        if self.logged_in && matches!(self.licences, Licences::Idle) && self.licence_possible() {
            self.licences = Licences::Loading;
            self.send(Cmd::Licences);
        }
        widgets::page_header(
            ui,
            t!("wizard.title"),
            "",
            None,
            |_| {},
        );

        let mut bar_top = None;
        if !self.rows.is_empty() {
            let plan = self.wizard_plan();
            let can_install = self.can_install();
            let bar = egui::TopBottomPanel::bottom("wizard_bar")
                .show_separator_line(false)
                .frame(Frame::new().fill(BG).inner_margin(Margin { left: 0, right: widgets::RIGHT_GUTTER as i8, top: BAR_TOP as i8, bottom: 0 }))
                .exact_height(65.0)
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(32.0);
                        let blocker = self.licence_blocker();
                        let summary = if let Some(b) = blocker {
                            b.to_string()
                        } else if plan.is_empty() {
                            t!("wizard.select_one").to_string()
                        } else {
                            let n = plan.modules.len() + usize::from(plan.editor);
                            let extras = usize::from(plan.hasp) + usize::from(plan.vcredist) + usize::from(plan.licence.is_some());
                            let count = |key: &'static str, n: usize| i18n::fill(i18n::plural(key, n as u64), &[("n", n.to_string())]);
                            if n == 0 {
                                count("wizard.selected_extras", extras)
                            } else if extras > 0 {
                                t!("wizard.selected_both", packages = count("wizard.selected_packages", n), extras = count("wizard.extras_count", extras))
                            } else {
                                count("wizard.selected_packages", n)
                            }
                        };
                        ui.label(RichText::new(summary).size(14.0).color(MUTED));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let ok = !plan.is_empty() && can_install && blocker.is_none();
                            if widgets::button(ui, Some(icon::PLAY), t!("wizard.start"), Btn::Primary, ok).clicked() {
                                self.actions.push(Action::Ask(Confirm::CleanInstall(plan.clone())));
                            }
                        });
                    });
                });
            bar_top = Some(bar.response.rect);
        }

        egui::CentralPanel::default().frame(Frame::new()).show_inside(ui, |ui| {
            widgets::scroll_page(ui, "wizard_scroll", |ui| self.ui_wizard_body(ui));
        });
        if let Some(r) = bar_top {
            ui.painter().with_clip_rect(r.expand(2.0)).hline(
                r.left()..=r.right() - widgets::RIGHT_GUTTER,
                r.top() + 0.5,
                Stroke::new(1.0_f32, BORDER),
            );
        }
    }

    fn ui_wizard_body(&mut self, ui: &mut egui::Ui) {
        self.ui_operation(ui);
        self.ui_result(ui);

        if self.rows.is_empty() {
            widgets::card(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.checking {
                        widgets::spinner(ui, 16.0);
                    }
                    ui.label(RichText::new(t!("wizard.loading_packages")).color(MUTED));
                });
            });
            return;
        }

        let enabled = !self.busy();
        let editor_row = self.rows.iter().find(|r| r.is_editor()).cloned();

        widgets::card(ui, |ui| {
            widgets::card_title(ui, t!("wizard.editor.title"));
            ui.add_space(12.0);
            match &editor_row {
                Some(row) if row.latest.is_some() => {
                    App::wizard_item(ui, &mut self.wizard.editor, enabled, "GeoStore V6", row);
                    if row.detected {
                        widgets::indented(ui, |ui| {
                            widgets::muted(ui, t!("wizard.editor.reinstall_note"));
                        });
                    }
                }
                _ => widgets::muted(ui, t!("wizard.editor.not_found")),
            }
        });
        ui.add_space(12.0);

        let modules: Vec<_> = self.rows.iter().filter(|r| !r.is_editor()).cloned().collect();
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::card_title(ui, t!("wizard.modules.title"));
                let all = modules.iter().all(|r| self.wizard.modules.get(&r.key).copied().unwrap_or(false));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = if all { t!("wizard.modules.deselect_all") } else { t!("wizard.modules.select_all") };
                    let link = ui.add_enabled(enabled, egui::Button::new(RichText::new(text).size(13.0).color(ACCENT)).frame(false));
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        for r in &modules {
                            self.wizard.modules.insert(r.key.clone(), !all);
                        }
                    }
                });
            });
            ui.add_space(12.0);
            let count = modules.len();
            for (i, row) in modules.iter().enumerate() {
                if row.complete.is_none() && row.latest.is_none() {
                    continue;
                }
                let checked = self.wizard.modules.entry(row.key.clone()).or_insert(false);
                App::wizard_item(ui, checked, enabled, &row.info.name, row);
                widgets::indented(ui, |ui| {
                    ui.label(RichText::new(&row.info.description).size(13.0).color(MUTED));
                });
                if i + 1 < count {
                    ui.add_space(8.0);
                }
            }
        });
        ui.add_space(12.0);

        widgets::card(ui, |ui| {
            widgets::card_title(ui, t!("wizard.extras.title"));
            ui.add_space(12.0);
            widgets::checkbox(
                ui,
                &mut self.wizard.hasp,
                RichText::new(t!("wizard.extras.hasp")).size(14.0),
                enabled,
            );
            ui.add_space(12.0);
            let vc_enabled = enabled && self.wizard.editor;
            let vc = widgets::checkbox(
                ui,
                &mut self.wizard.vcredist,
                RichText::new(t!("wizard.extras.vcredist")).size(14.0),
                vc_enabled,
            );
            if !self.wizard.editor {
                vc.on_hover_text(t!("wizard.extras.vcredist_hint"));
            }
            ui.add_space(12.0);
            let lic_enabled = enabled && self.licence_possible() && !self.no_licences();
            let mut lic_on = self.wizard.licence && !self.no_licences();
            let lic = widgets::checkbox(ui, &mut lic_on, RichText::new(t!("wizard.extras.licence")).size(14.0), lic_enabled);
            if lic.changed() {
                self.wizard.licence = lic_on;
            }
            if !self.licence_possible() {
                lic.on_hover_text(t!("wizard.extras.licence_hint"));
            } else {
                widgets::indented(ui, |ui| {
                    ui.add_space(8.0);
                    let choice_enabled = lic_enabled && self.wizard.licence;
                    self.ui_licence_choice(ui, choice_enabled);
                    self.ui_licman_account(ui, enabled && (choice_enabled || self.no_licences()));
                });
            }
        });
        ui.add_space(8.0);
    }
}
