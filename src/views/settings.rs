use crate::app::{Action, App};
use crate::fmt;
use crate::installer;
use crate::paths;
use crate::theme::*;
use crate::widgets::{self, icon, Btn};
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke};

const INTERVALS: [u32; 6] = [1, 2, 4, 8, 12, 24];
const VALUE_COL: f32 = 200.0;

fn hours_label(h: u32) -> String {
    match h {
        1 => "1 hodinu".into(),
        2..=4 => format!("{h} hodiny"),
        _ => format!("{h} hodin"),
    }
}

fn combo<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 32.0;
        ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
        add(ui)
    })
    .inner
}

fn field_row(ui: &mut egui::Ui, name: &str, value: impl Into<egui::WidgetText>) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(VALUE_COL, 24.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(VALUE_COL);
            ui.label(RichText::new(name).size(14.0).color(MUTED));
        });
        ui.label(value);
    });
}

fn label(text: &str) -> RichText {
    RichText::new(text).size(14.0)
}

impl App {
    pub(crate) fn ui_settings(&mut self, ui: &mut egui::Ui) {
        widgets::page_header(ui, "Nastavení", "Změny se ukládají okamžitě", None, |_| {});
        let mut changed = false;
        let mut task_changed = false;

        widgets::scroll_page(ui, "settings_scroll", |ui| {
            widgets::card(ui, |ui| {
                widgets::card_title(ui, "Automatické aktualizace");
                ui.add_space(12.0);
                changed |= widgets::checkbox(ui, &mut self.cfg.auto_check, label("Automaticky kontrolovat dostupnost aktualizací"), true).changed();
                widgets::indented(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(32.0);
                        ui.label(RichText::new("Při spuštěné aplikaci kontrolovat každé").size(13.0).color(MUTED));
                        ui.add_enabled_ui(self.cfg.auto_check, |ui| {
                            combo(ui, |ui| {
                                egui::ComboBox::from_id_salt("interval")
                                    .width(110.0)
                                    .icon(widgets::chevron)
                                    .selected_text(hours_label(self.cfg.check_interval_hours))
                                    .show_ui(ui, |ui| {
                                        for h in INTERVALS {
                                            changed |= ui.selectable_value(&mut self.cfg.check_interval_hours, h, hours_label(h)).changed();
                                        }
                                    });
                            });
                        });
                    });
                });
                ui.add_space(8.0);
                changed |= widgets::checkbox(ui, &mut self.cfg.auto_install, label("Nalezené aktualizace instalovat automaticky"), true).changed();
                widgets::indented(ui, |ui| {
                    widgets::muted(ui, "Instalace se spustí 15 sekund po nalezení aktualizace a lze ji ještě zrušit. Pokud je GeoStore V6 spuštěn, počká se na jeho ukončení.");
                });
                ui.add_space(8.0);
                if widgets::checkbox(ui, &mut self.cfg.run_at_logon, label("Kontrolovat při přihlášení do Windows a jednou denně"), true).changed() {
                    task_changed = true;
                }
                widgets::indented(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(32.0);
                        ui.label(RichText::new("Čas denní kontroly").size(13.0).color(MUTED));
                        ui.add_enabled_ui(self.cfg.run_at_logon, |ui| {
                            combo(ui, |ui| {
                                egui::ComboBox::from_id_salt("hour")
                                    .width(80.0)
                                    .icon(widgets::chevron)
                                    .selected_text(format!("{:02}:00", self.cfg.daily_check_hour))
                                    .show_ui(ui, |ui| {
                                        for h in 0..24u32 {
                                            if ui.selectable_value(&mut self.cfg.daily_check_hour, h, format!("{h:02}:00")).changed() {
                                                task_changed = true;
                                            }
                                        }
                                    });
                            });
                        });
                    });
                    if self.task_installed {
                        widgets::mark_label(ui, widgets::Mark::Success, "Naplánovaná úloha „GeoStore Updater“ je aktivní", 13.0);
                        if let Ok(exe) = std::env::current_exe() {
                            ui.label(widgets::soft_mono_text(exe.display().to_string(), 12.5, SUBTLE));
                            widgets::muted(ui, "Program po zapnutí úlohy nepřesouvejte");
                        }
                    }
                    if let Some(err) = &self.task_error {
                        ui.label(RichText::new(err).size(13.0).color(DANGER));
                    }
                });
                if (self.cfg.auto_check || self.cfg.run_at_logon) && !self.creds_stored {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(WARN_SOFT)
                        .stroke(Stroke::new(1.0_f32, WARN_BORDER))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin { left: 14, right: 8, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.set_min_height(32.0);
                                ui.spacing_mut().item_spacing.x = 10.0;
                                ui.label(widgets::icon_text(icon::WARNING, 14.0, WARN));
                                ui.label(
                                    RichText::new("Automatická kontrola po spuštění aplikace a naplánovaná úloha vyžadují uložené přihlášení")
                                        .size(13.0)
                                        .color(TEXT),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if widgets::button(ui, None, "Uložit přihlášení", Btn::Secondary, true).clicked() {
                                        self.actions.push(Action::SaveCreds);
                                    }
                                });
                            });
                        });
                }
            });
            ui.add_space(12.0);

            widgets::card(ui, |ui| {
                widgets::card_title(ui, "Instalace");
                ui.add_space(12.0);
                changed |= widgets::checkbox(ui, &mut self.cfg.backup_konfig, widgets::path_job("Před přepsáním zálohovat upravené soubory v C:\\V6-KONFIG", 14.0, TEXT), true).changed();
                widgets::indented(ui, |ui| {
                    ui.horizontal(|ui| {
                        widgets::muted(ui, &format!("Zálohy se ukládají do {}", paths::BACKUP_ROOT));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::button(ui, Some(icon::FOLDER), "Otevřít složku", Btn::Secondary, true).clicked() {
                                App::open(paths::BACKUP_ROOT);
                            }
                        });
                    });
                });
                ui.add_space(8.0);
                changed |= widgets::checkbox(ui, &mut self.cfg.keep_downloads, label("Ponechat stažené instalační balíčky"), true).changed();
                let dl = paths::downloads_dir();
                let size: u64 = installer::walk_files(&dl)
                    .iter()
                    .filter_map(|p| std::fs::metadata(p).ok())
                    .map(|m| m.len())
                    .sum();
                widgets::indented(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(widgets::prose_path_job("", &dl.display().to_string(), &format!(" · {}", fmt::mb(size)), 13.0, MUTED)).wrap());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if size > 0 && widgets::button(ui, Some(icon::DELETE), "Smazat", Btn::Secondary, !self.busy()).clicked() {
                                for f in installer::walk_files(&dl) {
                                    let _ = std::fs::remove_file(f);
                                }
                            }
                            if widgets::button(ui, Some(icon::FOLDER), "Otevřít složku", Btn::Secondary, true).clicked() {
                                App::open(&dl);
                            }
                        });
                    });
                });
            });
            ui.add_space(12.0);

            widgets::card(ui, |ui| {
                widgets::card_title(ui, "Sledované balíčky");
                ui.add_space(4.0);
                widgets::muted(ui, "Sledované balíčky se kontrolují a aktualizují. Každý balíček se vždy instaluje ze svého vlastního instalačního souboru.");
                ui.add_space(12.0);
                if self.rows.is_empty() {
                    widgets::muted(ui, "Seznam balíčků zatím nebyl načten");
                }
                let rows = self.rows.clone();
                for row in rows {
                    let mut on = self.cfg.is_tracked(&row.key, row.detected);
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(VALUE_COL, 24.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(VALUE_COL);
                            if widgets::checkbox(ui, &mut on, label(&row.info.name), true).changed() {
                                self.actions.push(Action::SetTracked(row.key.clone(), on));
                            }
                        });
                        match &row.local_version {
                            Some(v) if row.detected => {
                                ui.label(widgets::soft_mono_text(v, 13.0, MONO_INK));
                            }
                            _ => {
                                ui.label(RichText::new("nenalezeno v počítači").size(13.0).color(WARN));
                            }
                        }
                    });
                }
            });
            ui.add_space(12.0);

            widgets::card(ui, |ui| {
                widgets::card_title(ui, "Účet");
                ui.add_space(12.0);
                field_row(ui, "Uživatel", RichText::new(&self.user).size(14.0).color(TEXT));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(VALUE_COL, 24.0), Layout::left_to_right(Align::Center), |ui| {
                            ui.set_width(VALUE_COL);
                            ui.label(RichText::new("Přihlášení").size(14.0).color(MUTED));
                        });
                        if self.creds_stored {
                            widgets::mark_label(ui, widgets::Mark::Success, "Uloženo ve Správci pověření Windows", 14.0);
                        } else {
                            widgets::mark_label(ui, widgets::Mark::Empty, "Neuloženo", 14.0);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.creds_stored {
                            if widgets::button(ui, Some(icon::DELETE), "Odebrat uložené přihlášení", Btn::Secondary, true).clicked() {
                                self.actions.push(Action::ForgetCreds);
                            }
                        } else if widgets::button(ui, None, "Uložit přihlášení", Btn::Secondary, true).clicked() {
                            self.actions.push(Action::SaveCreds);
                        }
                    });
                });
            });
            ui.add_space(12.0);

            widgets::card(ui, |ui| {
                widgets::card_title(ui, "O aplikaci");
                ui.add_space(12.0);
                field_row(ui, "Verze", widgets::soft_mono_text(env!("CARGO_PKG_VERSION"), 13.0, MONO_INK));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    field_row(ui, "Data aplikace", widgets::soft_mono_text(paths::data_dir().display().to_string(), 13.0, MONO_INK));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, Some(icon::FOLDER), "Otevřít složku", Btn::Secondary, true).clicked() {
                            App::open(paths::data_dir());
                        }
                    });
                });
            });
            ui.add_space(28.0);
        });

        if changed {
            self.cfg.save();
        }
        if task_changed {
            self.actions.push(Action::ApplyTask);
        }
    }
}
