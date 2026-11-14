use crate::app::{Action, App};
use crate::t;
use crate::theme::*;
use crate::widgets::{self, icon, Btn};
use eframe::egui::{self, Frame, Key, RichText};

impl App {
    pub(crate) fn ui_login(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG))
            .show(ctx, |ui| {
                let width = 400.0;
                let height = 510.0;
                ui.add_space(((ui.available_height() - height) / 2.0).max(24.0));
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - width) / 2.0).max(16.0));
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        widgets::card(ui, |ui| self.login_card(ui));
                    });
                });
            });
        egui::Area::new(egui::Id::new("login_language"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20.0, 20.0))
            .show(ctx, |ui| {
                if let Some(lang) = widgets::language_picker(ui, self.cfg.lang()) {
                    self.set_language(lang);
                }
            });
    }

    fn field_label(ui: &mut egui::Ui, text: &str) {
        ui.label(RichText::new(text).size(13.0).color(MUTED));
        ui.add_space(6.0);
    }

    fn login_card(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.image((self.logo.id(), egui::vec2(48.0, 48.0)));
            ui.add_space(16.0);
            ui.label(RichText::new("GeoStoreV6 Updater").size(22.0).color(TEXT));
            ui.add_space(4.0);
            ui.label(RichText::new(t!("login.subtitle")).size(13.5).color(MUTED));
        });
        ui.add_space(24.0);

        let busy = self.login.busy;
        App::field_label(ui, t!("login.username"));
        let user = ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.login.user)
                .desired_width(f32::INFINITY)
                .margin(egui::vec2(12.0, 9.0)),
        );
        ui.add_space(20.0);
        App::field_label(ui, t!("login.password"));
        let pass = ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.login.pass)
                .password(!self.login.show_pass)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin { left: 12, right: 40, top: 9, bottom: 9 }),
        );
        let eye = egui::Rect::from_center_size(egui::pos2(ui.max_rect().right() - 20.0, pass.rect.center().y), egui::vec2(28.0, 28.0));
        let (glyph, tip) = if self.login.show_pass { (icon::HIDE, t!("login.hide_password")) } else { (icon::VIEW, t!("login.show_password")) };
        let toggled = ui
            .allocate_new_ui(egui::UiBuilder::new().max_rect(eye), |ui| widgets::button_ex(ui, Some(glyph), "", Btn::Ghost, !busy, 28.0, 28.0))
            .inner
            .on_hover_text(tip)
            .clicked();
        if toggled {
            self.login.show_pass = !self.login.show_pass;
        }
        if self.login.user.is_empty() && !busy && !user.has_focus() && !pass.has_focus() {
            user.request_focus();
        }
        let enter = (user.lost_focus() || pass.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(20.0);
        widgets::checkbox(ui, &mut self.login.remember, RichText::new(t!("login.remember")).size(14.0), !busy);

        if let Some(err) = self.login.error.clone() {
            ui.add_space(12.0);
            widgets::banner(ui, widgets::Tone::Danger, t!("login.failed"), &err, 0.0, |_| {});
        } else {
            ui.add_space(20.0);
        }

        if busy {
            ui.horizontal(|ui| {
                ui.set_height(36.0);
                ui.add_space((ui.available_width() - 120.0) / 2.0);
                widgets::spinner(ui, 16.0);
                ui.add_space(8.0);
                ui.label(RichText::new(t!("login.signing_in")).size(14.0).color(MUTED));
            });
        } else {
            let w = ui.available_width();
            let filled = !self.login.user.trim().is_empty() && !self.login.pass.is_empty();
            let clicked = widgets::button_ex(ui, None, t!("login.sign_in"), Btn::Primary, filled, w, 36.0).clicked();
            if clicked || (enter && filled) {
                self.actions.push(Action::Login);
            }
        }
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(t!("login.disclaimer")).size(12.5).color(MUTED));
        });
        ui.add_space(4.0);
    }
}
