use crate::app::{Action, App, Confirm};
use crate::fmt;
use crate::logic::{PackageRow, Status};
use crate::theme::*;
use crate::widgets::{self, icon, Btn, Tone};
use crate::worker::CleanPlan;
use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Stroke};

impl App {
    fn sync_wizard(&mut self) {
        if self.wizard.initialized || self.rows.is_empty() {
            return;
        }
        let editor_installed = self.rows.iter().any(|r| r.is_editor() && r.detected);
        self.wizard.editor = !editor_installed;
        self.wizard.hasp = !editor_installed;
        self.wizard.vcredist = !editor_installed;
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
        }
    }

    fn wizard_item(ui: &mut egui::Ui, checked: &mut bool, enabled: bool, name: &str, row: &PackageRow) {
        ui.horizontal(|ui| {
            widgets::checkbox(ui, checked, widgets::semibold_text(name, 14.0, if enabled { TEXT } else { DISABLED_TEXT }), enabled);
            if row.detected {
                let (text, color) = match row.status {
                    Status::UpdateAvailable => ("Nainstalováno, na portálu je novější", DANGER),
                    _ => ("Nainstalováno, aktuální", SUCCESS),
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
        widgets::page_header(
            ui,
            "Nová instalace",
            "Instalace grafického editoru a vybraných aplikačních řešení na tento počítač",
            None,
            |_| {},
        );

        let mut bar_top = None;
        if !self.rows.is_empty() {
            let plan = self.wizard_plan();
            let can_install = self.can_install();
            let bar = egui::TopBottomPanel::bottom("wizard_bar")
                .show_separator_line(false)
                .frame(Frame::new().fill(BG).inner_margin(Margin { left: 0, right: widgets::RIGHT_GUTTER as i8, top: 0, bottom: 0 }))
                .exact_height(64.0)
                .show_inside(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        let summary = if plan.is_empty() {
                            "Vyberte alespoň jednu položku".to_string()
                        } else {
                            let n = plan.modules.len() + usize::from(plan.editor);
                            let extras = usize::from(plan.hasp) + usize::from(plan.vcredist);
                            let verb = match n {
                                1 => "Vybrán",
                                2..=4 => "Vybrány",
                                _ => "Vybráno",
                            };
                            let mut s = format!("{verb} {n} {}", fmt::packages(n));
                            if extras > 0 {
                                s.push_str(&format!(" · {extras} {}", if extras == 1 { "doplněk" } else { "doplňky" }));
                            }
                            s
                        };
                        ui.label(RichText::new(summary).size(14.0).color(MUTED));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let ok = !plan.is_empty() && can_install;
                            if widgets::button(ui, Some(icon::PLAY), "Spustit instalaci", Btn::Primary, ok).clicked() {
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
                    ui.label(RichText::new("Seznam balíčků se načítá z portálu…").color(MUTED));
                });
            });
            return;
        }

        let enabled = !self.busy();
        let editor_row = self.rows.iter().find(|r| r.is_editor()).cloned();

        widgets::card(ui, |ui| {
            widgets::card_title(ui, "1. Grafický editor");
            ui.add_space(12.0);
            match &editor_row {
                Some(row) if row.latest.is_some() => {
                    let f = row.latest.as_ref().unwrap();
                    App::wizard_item(ui, &mut self.wizard.editor, enabled, "GeoStore V6", row);
                    widgets::indented(ui, |ui| {
                        ui.label(RichText::new("Základní grafický editor").size(13.0).color(MUTED));
                        ui.label(widgets::soft_mono_text(format!("{} · {}", f.title, fmt::date(f.date)), 12.5, SECONDARY_INK));
                        if row.detected {
                            ui.add_space(4.0);
                            widgets::muted(
                                ui,
                                &format!(
                                    "Nainstalována verze {}. Pokud editor vyberete, bude odinstalován a nainstalován znovu.",
                                    row.local_version.clone().unwrap_or_default()
                                ),
                            );
                        }
                    });
                }
                _ => widgets::muted(ui, "Instalační balíček editoru nebyl na portálu nalezen"),
            }
        });
        ui.add_space(12.0);

        let modules: Vec<_> = self.rows.iter().filter(|r| !r.is_editor()).cloned().collect();
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::card_title(ui, "2. Aplikační řešení");
                let all = modules.iter().all(|r| self.wizard.modules.get(&r.key).copied().unwrap_or(false));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let text = if all { "Zrušit výběr" } else { "Vybrat vše" };
                    let link = ui.add_enabled(enabled, egui::Button::new(RichText::new(text).size(13.0).color(ACCENT)).frame(false));
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        for r in &modules {
                            self.wizard.modules.insert(r.key.clone(), !all);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            widgets::muted(ui, "Použijí se kompletní instalační balíčky včetně výchozího nastavení prostředí a zástupců na ploše.");
            ui.add_space(12.0);
            let count = modules.len();
            for (i, row) in modules.iter().enumerate() {
                let file = row.complete.clone().or_else(|| row.latest.clone());
                let Some(file) = file else { continue };
                let checked = self.wizard.modules.entry(row.key.clone()).or_insert(false);
                App::wizard_item(ui, checked, enabled, &row.info.name, row);
                widgets::indented(ui, |ui| {
                    ui.label(RichText::new(&row.info.description).size(13.0).color(MUTED));
                    ui.add(egui::Label::new(widgets::soft_mono_text(format!("{} · {}", file.title, fmt::date(file.date)), 12.5, SECONDARY_INK)).truncate());
                });
                if i + 1 < count {
                    ui.add_space(8.0);
                }
            }
        });
        ui.add_space(12.0);

        widgets::card(ui, |ui| {
            widgets::card_title(ui, "3. Doplňky");
            ui.add_space(12.0);
            widgets::checkbox(
                ui,
                &mut self.wizard.hasp,
                RichText::new("Nainstalovat ovladač hardwarového klíče (Sentinel HASP)").size(14.0),
                enabled,
            );
            widgets::indented(ui, |ui| {
                widgets::muted(ui, "Spustí se průvodce instalací ovladače z instalačního balíčku. Bez ovladače nelze GeoStore V6 s HW klíčem spustit.");
            });
            ui.add_space(8.0);
            let vc_enabled = enabled && self.wizard.editor;
            widgets::checkbox(
                ui,
                &mut self.wizard.vcredist,
                RichText::new("Doinstalovat Microsoft Visual C++ 2010 a 2012 (x64)").size(14.0),
                vc_enabled,
            );
            widgets::indented(ui, |ui| {
                let text = if self.wizard.editor {
                    "Knihovny potřebné pro spuštění GeoStore V6 a práci s rastry. Instalují se tiše z adresáře Redist editoru."
                } else {
                    "Dostupné jen při instalaci editoru. Knihovny se instalují z jeho adresáře Redist."
                };
                ui.add(egui::Label::new(widgets::path_job(text, 13.0, if vc_enabled { MUTED } else { DISABLED_TEXT })).wrap());
            });
        });
        ui.add_space(12.0);

        widgets::banner(
            ui,
            Tone::Warn,
            "Co kompletní instalace přepíše",
            "Konfigurační soubory v C:\\V6-KONFIG, nastavení prostředí v %LOCALAPPDATA%\\GEOVAP\\GSV6 a zástupce na ploše. Konfigurační soubory, které se liší od dodávaných, se před přepsáním zálohují do C:\\V6-KONFIG_zalohy.",
            0.0,
            |_| {},
        );
        ui.add_space(8.0);
    }
}
