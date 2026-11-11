use crate::catalog::EDITOR;
use crate::config::Config;
use crate::fmt;
use crate::logfile::{self, LogLine};
use crate::logic::{self, PackageRow, Status};
use crate::portal::Licence;
use crate::theme::{self, *};
use crate::widgets::{self, icon, Btn};
use crate::worker::{self, CleanPlan, Cmd, Event, InstallOptions, Progress};
use crate::{creds, local, paths, scheduler};
use chrono::{DateTime, Local};
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const AUTO_INSTALL_DELAY: Duration = Duration::from_secs(15);
const MAX_LOG_LINES: usize = 6000;

#[derive(Default, Clone, Copy)]
pub struct Launch {
    pub background: bool,
    pub relaunch: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Wizard,
    History,
    Settings,
}

#[derive(Default)]
pub struct LoginForm {
    pub user: String,
    pub pass: String,
    pub remember: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub show_pass: bool,
}

#[derive(Clone)]
pub struct OpUi {
    pub title: String,
    pub progress: Option<Progress>,
    pub waiting_geostore: bool,
    pub keys: Vec<String>,
    pub done: Vec<String>,
}

impl OpUi {
    pub fn new(title: impl Into<String>, keys: Vec<String>) -> Self {
        Self { title: title.into(), progress: None, waiting_geostore: false, keys, done: Vec::new() }
    }
}

#[derive(Clone, PartialEq)]
pub enum Confirm {
    UpdateAll(Vec<String>),
    Reinstall(String),
    InstallModule(String),
    CleanInstall(CleanPlan),
    Logout,
    CancelOp,
}

/// Install request carried across the UAC restart, so the elevated copy picks up where the user left off.
#[derive(serde::Serialize, serde::Deserialize)]
enum Resume {
    UpdateAll(Vec<String>),
    Install(Vec<String>),
    Reinstall(String),
    InstallModule(String),
    CleanInstall(CleanPlan),
    ApplyTask,
}

impl Resume {
    fn path() -> PathBuf {
        paths::data_dir().join("resume.json")
    }

    fn take(relaunched: bool) -> Option<Self> {
        let path = Self::path();
        let text = std::fs::read_to_string(&path).ok();
        let _ = std::fs::remove_file(&path);
        if !relaunched {
            return None;
        }
        serde_json::from_str(&text?).ok()
    }

    fn save(&self) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(), json);
        }
    }
}

#[derive(Default)]
pub struct WizardState {
    pub initialized: bool,
    pub editor: bool,
    pub modules: BTreeMap<String, bool>,
    pub hasp: bool,
    pub vcredist: bool,
    pub licence: bool,
    pub licence_guid: Option<String>,
}

#[derive(Default)]
pub enum Licences {
    #[default]
    Idle,
    Loading,
    Ready(Vec<Licence>),
    Failed(String),
}

pub enum Action {
    Login,
    Check,
    Install(Vec<String>, bool),
    CleanInstall(CleanPlan),
    MarkCurrent(String),
    SetTracked(String, bool),
    Cancel,
    Ask(Confirm),
    Navigate(View),
    DismissResult,
    InstallNow,
    DismissCountdown,
    ForgetCreds,
    SaveCreds,
    ApplyTask,
}

pub struct App {
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    pub cfg: Config,
    pub logo: egui::TextureHandle,
    pub elevated: bool,
    pub logged_in: bool,
    pub user: String,
    pub login: LoginForm,
    pub view: View,
    pub rows: Vec<PackageRow>,
    pub last_check: Option<DateTime<Local>>,
    pub checking: bool,
    pub check_error: Option<String>,
    pub op: Option<OpUi>,
    pub result: Option<(bool, String)>,
    pub log: Vec<LogLine>,
    pub show_detail: bool,
    pub geostore_running: bool,
    last_poll: Instant,
    pub background_launch: bool,
    pub countdown: Option<Instant>,
    countdown_dismissed: bool,
    pub wizard: WizardState,
    pub licences: Licences,
    pub confirm: Option<Confirm>,
    pub actions: Vec<Action>,
    pub task_installed: bool,
    pub task_error: Option<String>,
    pub creds_stored: bool,
    auto_login_started: bool,
    preview_confirm: bool,
    title_styled: bool,
    close_requested: bool,
    resume: Option<Resume>,
    last_auto_check: Instant,
    pub save_creds: Option<String>,
}

pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        theme::setup(&cc.egui_ctx);
        let icon = eframe::icon_data::from_png_bytes(ICON_PNG).unwrap_or_default();
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba),
            egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear)),
        );
        let (tx, rx, cancel) = worker::spawn(cc.egui_ctx.clone());
        let cfg = Config::load();
        let stored = creds::load();
        let mut login = LoginForm {
            remember: cfg.remember_login,
            user: cfg.last_user.clone(),
            ..Default::default()
        };
        if let Some((u, _)) = &stored {
            login.user = u.clone();
        }
        let mut task_error = None;
        if cfg.run_at_logon {
            if let Err(e) = scheduler::install(cfg.daily_check_hour) {
                task_error = Some(worker::friendly_error(&e));
            }
        }
        let mut app = Self {
            tx,
            rx,
            cancel,
            logo,
            elevated: local::is_elevated(),
            logged_in: false,
            user: String::new(),
            login,
            view: View::Dashboard,
            rows: Vec::new(),
            last_check: None,
            checking: false,
            check_error: None,
            op: None,
            result: None,
            log: logfile::read_tail(400),
            show_detail: false,
            geostore_running: local::is_geostore_running(),
            last_poll: Instant::now(),
            background_launch: launch.background,
            countdown: None,
            countdown_dismissed: false,
            wizard: WizardState::default(),
            licences: Licences::Idle,
            confirm: None,
            actions: Vec::new(),
            task_installed: scheduler::is_installed(),
            task_error,
            creds_stored: stored.is_some(),
            auto_login_started: false,
            preview_confirm: false,
            title_styled: false,
            close_requested: false,
            resume: Resume::take(launch.relaunch),
            last_auto_check: Instant::now(),
            save_creds: None,
            cfg,
        };
        if cfg!(debug_assertions) {
            app.apply_preview();
        }
        app
    }

    fn preview_licences(list: &mut Vec<Licence>) {
        let wanted = std::env::var("GSU_PREVIEW").is_ok_and(|m| m.split(',').any(|o| o.trim() == "licences"));
        let Some(first) = list.first().cloned().filter(|_| wanted) else { return };
        for (i, (project, valid, trial)) in [("GSV6 - Pobočka Zlín", "31.12.2027", false), ("GSV6 - zkušební", "30.11.2026", true)].into_iter().enumerate() {
            let mut l = first.clone();
            l.guid = format!("{}-preview{i}", l.guid);
            l.project = project.into();
            l.valid_to = valid.into();
            l.trial = trial;
            list.push(l);
        }
    }

    /// Debug-only screenshot aid. GSU_PREVIEW is a comma list: admin (install controls on), op (sample progress card), confirm (open the update dialog), licences (two extra fake licences), view=wizard|history|settings.
    fn apply_preview(&mut self) {
        let Ok(mode) = std::env::var("GSU_PREVIEW") else { return };
        self.elevated = true;
        let opts: Vec<&str> = mode.split(',').map(str::trim).collect();
        self.preview_confirm = opts.contains(&"confirm");
        for o in &opts {
            match o.strip_prefix("view=") {
                Some("wizard") => self.view = View::Wizard,
                Some("history") => self.view = View::History,
                Some("settings") => self.view = View::Settings,
                _ => {}
            }
        }
        if opts.contains(&"op") {
            let mut op = OpUi::new("Instalace aktualizací", vec!["DTMCR".into(), "IG".into()]);
            op.done.push("V6-DTMČR".into());
            op.progress = Some(Progress {
                step: 2,
                steps: 2,
                package: "V6-IG".into(),
                stage: "Stahování castecna_instalace_aplikace_IG.zip".into(),
                fraction: Some(0.42),
                detail: "135,7 MB z 323,4 MB · 7,8 MB/s · zbývá asi 24 s".into(),
                cancellable: true,
            });
            self.op = Some(op);
        }
    }

    pub(crate) fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn tracked_keys(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| self.cfg.is_tracked(&r.key, r.detected))
            .map(|r| r.key.clone())
            .collect()
    }

    pub fn pending_keys(&self) -> Vec<String> {
        logic::pending_keys(&self.rows, &self.cfg)
    }

    /// Package finished in the running operation; its row shows the new state before the re-check arrives.
    pub fn finished_in_op(&self, row: &PackageRow) -> bool {
        self.op.as_ref().is_some_and(|op| op.keys.contains(&row.key) && op.done.contains(&row.info.name))
    }

    pub fn row(&self, key: &str) -> Option<&PackageRow> {
        self.rows.iter().find(|r| r.key == key)
    }

    pub fn busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn can_install(&self) -> bool {
        self.op.is_none()
    }

    fn admin_request(action: &Action) -> Option<Resume> {
        match action {
            Action::Ask(Confirm::UpdateAll(k)) => Some(Resume::UpdateAll(k.clone())),
            Action::Ask(Confirm::Reinstall(k)) => Some(Resume::Reinstall(k.clone())),
            Action::Ask(Confirm::InstallModule(k)) => Some(Resume::InstallModule(k.clone())),
            Action::Ask(Confirm::CleanInstall(p)) => Some(Resume::CleanInstall(p.clone())),
            Action::Install(k, _) => Some(Resume::Install(k.clone())),
            Action::CleanInstall(p) => Some(Resume::CleanInstall(p.clone())),
            Action::ApplyTask => Some(Resume::ApplyTask),
            _ => None,
        }
    }

    /// Restarts through the UAC prompt and hands the request to the elevated copy.
    fn elevate_for(&mut self, resume: Resume) {
        let task = matches!(resume, Resume::ApplyTask);
        self.cfg.save();
        resume.save();
        if local::relaunch_elevated() {
            self.close_requested = true;
            return;
        }
        let _ = std::fs::remove_file(Resume::path());
        if task {
            self.task_error = Some("Naplánovaná úloha vyžaduje práva správce, oprávnění nebylo uděleno.".into());
        } else {
            self.result = Some((false, "Instalace vyžaduje práva správce, oprávnění nebylo uděleno.".into()));
        }
    }

    fn resume_pending(&mut self) {
        if !self.logged_in || self.rows.is_empty() || self.checking || self.op.is_some() || self.confirm.is_some() {
            return;
        }
        let Some(resume) = self.resume.take() else { return };
        match resume {
            Resume::UpdateAll(keys) => {
                self.view = View::Dashboard;
                self.actions.push(Action::Ask(Confirm::UpdateAll(keys)));
            }
            Resume::Install(keys) => {
                self.view = View::Dashboard;
                self.actions.push(Action::Install(keys, false));
            }
            Resume::Reinstall(key) => {
                self.view = View::Dashboard;
                self.actions.push(Action::Ask(Confirm::Reinstall(key)));
            }
            Resume::InstallModule(key) => {
                self.view = View::Dashboard;
                self.actions.push(Action::Ask(Confirm::InstallModule(key)));
            }
            Resume::CleanInstall(plan) => {
                self.wizard = WizardState {
                    initialized: true,
                    editor: plan.editor,
                    modules: self
                        .rows
                        .iter()
                        .filter(|r| !r.is_editor())
                        .map(|r| (r.key.clone(), plan.modules.contains(&r.key)))
                        .collect(),
                    hasp: plan.hasp,
                    vcredist: plan.vcredist,
                    licence: plan.licence.is_some(),
                    licence_guid: plan.licence.clone(),
                };
                self.view = View::Wizard;
                self.actions.push(Action::Ask(Confirm::CleanInstall(plan)));
            }
            Resume::ApplyTask => {
                self.view = View::Settings;
                self.actions.push(Action::ApplyTask);
            }
        }
    }

    fn install_options(&self, force: bool) -> InstallOptions {
        InstallOptions {
            backup: self.cfg.backup_konfig,
            keep_downloads: self.cfg.keep_downloads,
            tracked: self.tracked_keys(),
            force,
        }
    }

    fn pump_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::LoggedIn { user } => {
                    self.logged_in = true;
                    self.user = user;
                    self.login.busy = false;
                    self.login.error = None;
                    self.login.pass.clear();
                    self.creds_stored = creds::exists();
                }
                Event::LoginFailed(e) => {
                    self.login.busy = false;
                    if self.logged_in {
                        self.result = Some((false, format!("Přihlášení se nepodařilo uložit\n{e}")));
                    } else {
                        self.login.error = Some(e);
                    }
                }
                Event::LoggedOut => {
                    self.logged_in = false;
                    self.licences = Licences::Idle;
                    self.rows.clear();
                    self.creds_stored = false;
                    self.countdown = None;
                }
                Event::Checking => self.checking = true,
                Event::Overview { rows, checked_at } => {
                    self.rows = rows;
                    self.last_check = checked_at;
                    self.checking = false;
                    self.check_error = None;
                    self.last_auto_check = Instant::now();
                    self.on_overview(ctx);
                }
                Event::CheckFailed(e) => {
                    self.checking = false;
                    self.check_error = Some(e);
                    self.last_auto_check = Instant::now();
                }
                Event::OpStarted { title } => {
                    let keys = self.op.take().map(|o| o.keys).unwrap_or_default();
                    self.op = Some(OpUi::new(title, keys));
                    self.result = None;
                }
                Event::Progress(p) => {
                    if let Some(op) = &mut self.op {
                        if let Some(prev) = &op.progress {
                            if prev.package != p.package && !op.done.contains(&prev.package) {
                                op.done.push(prev.package.clone());
                            }
                        }
                        op.progress = Some(p);
                    }
                }
                Event::WaitingForGeoStore(w) => {
                    if let Some(op) = &mut self.op {
                        op.waiting_geostore = w;
                    }
                }
                Event::Log(line) => {
                    self.log.push(line);
                    if self.log.len() > MAX_LOG_LINES {
                        let excess = self.log.len() - MAX_LOG_LINES;
                        self.log.drain(..excess);
                    }
                }
                Event::Licences(result) => {
                    self.licences = match result {
                        Ok(mut list) => {
                            if cfg!(debug_assertions) {
                                Self::preview_licences(&mut list);
                            }
                            let keep = self.wizard.licence_guid.as_ref().is_some_and(|g| list.iter().any(|l| &l.guid == g));
                            if !keep {
                                self.wizard.licence_guid = list.first().map(|l| l.guid.clone());
                            }
                            Licences::Ready(list)
                        }
                        Err(e) => Licences::Failed(e),
                    };
                }
                Event::OpFinished { ok, summary } => {
                    self.op = None;
                    self.result = Some((ok, summary));
                    self.cancel.store(false, Ordering::Relaxed);
                }
            }
        }
    }

    fn on_overview(&mut self, ctx: &egui::Context) {
        let pending = self.pending_keys();
        if pending.is_empty() {
            self.countdown = None;
            return;
        }
        if std::mem::take(&mut self.preview_confirm) {
            self.confirm = Some(Confirm::UpdateAll(pending.clone()));
        }
        if self.background_launch {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
        if self.cfg.auto_install && !self.countdown_dismissed && self.op.is_none() && self.elevated && self.countdown.is_none() {
            self.countdown = Some(Instant::now() + AUTO_INSTALL_DELAY);
        }
    }

    fn tick(&mut self, ctx: &egui::Context) {
        if !self.auto_login_started {
            self.auto_login_started = true;
            if let Some((user, pass)) = creds::load() {
                self.login.user = user.clone();
                self.login.busy = true;
                self.send(Cmd::Login { user, pass, remember: true });
            }
        }
        if self.last_poll.elapsed() > Duration::from_secs(3) {
            self.last_poll = Instant::now();
            self.geostore_running = local::is_geostore_running();
        }
        let interval = Duration::from_secs(self.cfg.check_interval_hours.max(1) as u64 * 3600);
        if self.logged_in && self.cfg.auto_check && !self.checking && self.op.is_none() && self.last_auto_check.elapsed() > interval {
            self.last_auto_check = Instant::now();
            self.checking = true;
            self.send(Cmd::Check);
        }
        if let Some(at) = self.countdown {
            if self.op.is_some() {
                self.countdown = None;
            } else if Instant::now() >= at {
                self.countdown = None;
                let keys = self.pending_keys();
                if !keys.is_empty() {
                    self.start_install(keys, false);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn start_install(&mut self, keys: Vec<String>, force: bool) {
        if !self.can_install() {
            return;
        }
        let opts = self.install_options(force);
        self.op = Some(OpUi::new("Instalace aktualizací", keys.clone()));
        self.result = None;
        self.send(Cmd::Install { keys, opts });
    }

    fn apply_actions(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            if !self.elevated {
                if let Some(resume) = Self::admin_request(&action) {
                    self.elevate_for(resume);
                    break;
                }
            }
            match action {
                Action::Login => {
                    if self.login.user.trim().is_empty() || self.login.pass.is_empty() {
                        self.login.error = Some("Vyplňte uživatelské jméno a heslo.".into());
                        continue;
                    }
                    self.login.busy = true;
                    self.login.error = None;
                    self.cfg.remember_login = self.login.remember;
                    self.cfg.last_user = self.login.user.trim().to_string();
                    self.cfg.save();
                    self.send(Cmd::Login {
                        user: self.login.user.trim().to_string(),
                        pass: self.login.pass.clone(),
                        remember: self.login.remember,
                    });
                }
                Action::Check => {
                    if !self.checking {
                        self.checking = true;
                        self.check_error = None;
                        self.send(Cmd::Check);
                    }
                }
                Action::Install(keys, force) => self.start_install(keys, force),
                Action::CleanInstall(plan) => {
                    if !self.can_install() {
                        continue;
                    }
                    for key in &plan.modules {
                        self.cfg.tracked.insert(key.clone(), true);
                    }
                    if plan.editor {
                        self.cfg.tracked.insert(EDITOR.into(), true);
                    }
                    self.cfg.save();
                    let mut opts = self.install_options(true);
                    for key in plan.modules.iter().chain(plan.editor.then(|| EDITOR.to_string()).iter()) {
                        if !opts.tracked.contains(key) {
                            opts.tracked.push(key.clone());
                        }
                    }
                    let mut keys = plan.modules.clone();
                    if plan.editor {
                        keys.insert(0, EDITOR.to_string());
                    }
                    self.op = Some(OpUi::new("Nová instalace", keys));
                    self.result = None;
                    self.send(Cmd::CleanInstall { plan, opts });
                }
                Action::MarkCurrent(key) => self.send(Cmd::MarkCurrent(key)),
                Action::SetTracked(key, on) => {
                    self.cfg.tracked.insert(key, on);
                    self.cfg.save();
                }
                Action::Cancel => self.cancel.store(true, Ordering::Relaxed),
                Action::Ask(c) => self.confirm = Some(c),
                Action::Navigate(v) => self.view = v,
                Action::DismissResult => self.result = None,
                Action::InstallNow => {
                    self.countdown = None;
                    let keys = self.pending_keys();
                    self.start_install(keys, false);
                }
                Action::DismissCountdown => {
                    self.countdown = None;
                    self.countdown_dismissed = true;
                }
                Action::ForgetCreds => {
                    creds::delete();
                    self.creds_stored = false;
                }
                Action::SaveCreds => self.save_creds = Some(String::new()),
                Action::ApplyTask => {
                    let result = if self.cfg.run_at_logon {
                        scheduler::install(self.cfg.daily_check_hour)
                    } else {
                        scheduler::uninstall()
                    };
                    self.task_error = result.err().map(|e| worker::friendly_error(&e));
                    self.task_installed = scheduler::is_installed();
                    self.cfg.save();
                }
            }
        }
    }

    fn confirm_ok(&mut self, c: Confirm) {
        match c {
            Confirm::UpdateAll(keys) => self.actions.push(Action::Install(keys, false)),
            Confirm::Reinstall(key) => self.actions.push(Action::Install(vec![key], true)),
            Confirm::InstallModule(key) => self.actions.push(Action::CleanInstall(CleanPlan {
                editor: false,
                modules: vec![key],
                hasp: false,
                vcredist: false,
                licence: None,
            })),
            Confirm::CleanInstall(plan) => self.actions.push(Action::CleanInstall(plan)),
            Confirm::Logout => self.send(Cmd::Logout),
            Confirm::CancelOp => self.actions.push(Action::Cancel),
        }
    }

    fn names(&self, keys: &[String]) -> String {
        keys.iter()
            .map(|k| self.row(k).map(|r| r.info.name.clone()).unwrap_or_else(|| k.clone()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn modal_frame() -> Frame {
        Frame::new()
            .fill(CARD)
            .stroke(egui::Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(Margin::same(24))
            .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(28) })
    }

    fn geostore_warning(&self) -> Option<(&'static str, &'static str)> {
        self.geostore_running.then_some((
            "GeoStore V6 je spuštěn",
            "Uložte práci a zavřete jej. Instalace na jeho ukončení počká.",
        ))
    }

    fn package_change_line(&self, key: &str) -> (String, String) {
        let Some(r) = self.row(key) else { return (key.to_string(), String::new()) };
        let from = r.installed_date.map(fmt::date).unwrap_or_else(|| "—".into());
        let to = r.latest.as_ref().map(|f| fmt::date(f.date)).unwrap_or_else(|| "—".into());
        (r.info.name.clone(), format!("{from} → {to}"))
    }

    fn ui_confirm(&mut self, ctx: &egui::Context) {
        let Some(c) = self.confirm.clone() else { return };
        let mut lines: Vec<(String, String)> = Vec::new();
        let (title, body, ok_label, warn): (String, String, &str, Option<(&str, &str)>) = match &c {
            Confirm::UpdateAll(keys) => {
                lines = keys.iter().map(|k| self.package_change_line(k)).collect();
                let title = if keys.len() == 1 {
                    format!("Aktualizovat {}?", self.names(keys))
                } else {
                    format!("Aktualizovat {} {}?", keys.len(), fmt::packages(keys.len()))
                };
                (title, String::new(), "Aktualizovat", self.geostore_warning())
            }
            Confirm::Reinstall(key) => (
                format!("Přeinstalovat {}?", self.names(std::slice::from_ref(key))),
                String::new(),
                "Přeinstalovat",
                self.geostore_warning(),
            ),
            Confirm::InstallModule(key) => (
                format!("Nainstalovat {}?", self.names(std::slice::from_ref(key))),
                String::new(),
                "Nainstalovat",
                Some(("Přepíše se nastavení prostředí", "Kompletní instalace přepíše nastavení v profilu uživatele (%LOCALAPPDATA%\\GEOVAP\\GSV6).")),
            ),
            Confirm::CleanInstall(plan) => {
                if plan.editor {
                    lines.push(("GeoStore V6".to_string(), String::new()));
                }
                lines.extend(plan.modules.iter().map(|k| (self.names(std::slice::from_ref(k)), String::new())));
                if let Some(guid) = &plan.licence {
                    let name = match &self.licences {
                        Licences::Ready(list) => list.iter().find(|l| &l.guid == guid).map(|l| l.title()),
                        _ => None,
                    };
                    lines.push(("Licence".to_string(), name.unwrap_or_else(|| "gsfl.lic z licman".into())));
                }
                (
                    "Spustit novou instalaci?".into(),
                    String::new(),
                    "Spustit instalaci",
                    Some(("Přepíšou se konfigurační soubory", "Kompletní instalace přepíše soubory v C:\\V6-KONFIG a nastavení prostředí v profilu uživatele. Upravené konfigurační soubory se zálohují.")),
                )
            }
            Confirm::CancelOp => (
                "Přerušit instalaci?".into(),
                "Rozpracovaný balíček se nedokončí a další balíčky ve frontě se nenainstalují. Již nainstalované balíčky zůstanou beze změny.".into(),
                "Přerušit instalaci",
                None,
            ),
            Confirm::Logout => (
                "Odhlásit se?".into(),
                "Uložené přihlašovací údaje budou odstraněny. Automatické aktualizace nebudou fungovat, dokud se znovu nepřihlásíte.".into(),
                "Odhlásit",
                None,
            ),
        };
        let mut close = false;
        let mut ok = false;
        let modal = egui::Modal::new(egui::Id::new("confirm")).frame(Self::modal_frame()).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(&title).size(20.0).color(TEXT));
            ui.add_space(16.0);
            if !body.is_empty() {
                ui.add(egui::Label::new(RichText::new(&body).size(14.0).color(MUTED)).wrap());
                ui.add_space(if lines.is_empty() { 16.0 } else { 8.0 });
            }
            if !lines.is_empty() {
                widgets::change_list(ui, &lines);
                ui.add_space(16.0);
            }
            match warn {
                Some((head, text)) => widgets::banner(ui, widgets::Tone::Warn, head, text, 0.0, |_| {}),
                None => {}
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let kind = if matches!(c, Confirm::Logout | Confirm::CancelOp) { Btn::Danger } else { Btn::Primary };
                let back = if c == Confirm::CancelOp { "Pokračovat" } else { "Zrušit" };
                if widgets::button_sized(ui, None, back, Btn::Secondary, true, 96.0).clicked() {
                    close = true;
                }
                if widgets::button_sized(ui, None, ok_label, kind, true, 96.0).clicked() {
                    ok = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                ok = true;
            }
        });
        if ok {
            self.confirm = None;
            self.confirm_ok(c);
        } else if close || modal.should_close() {
            self.confirm = None;
        }
    }

    fn ui_save_creds(&mut self, ctx: &egui::Context) {
        if self.save_creds.is_none() {
            return;
        }
        let mut ok = false;
        let mut close = false;
        let user = self.user.clone();
        let modal = egui::Modal::new(egui::Id::new("save_creds")).frame(Self::modal_frame()).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new("Uložit přihlášení").size(20.0).color(TEXT));
            ui.add_space(16.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Zadejte heslo k účtu {user}. Heslo se uloží do Správce pověření systému Windows a použije se pro automatickou kontrolu."))
                        .size(14.0)
                        .color(MUTED),
                )
                .wrap(),
            );
            ui.add_space(16.0);
            ui.label(RichText::new("Heslo").size(13.0).color(MUTED));
            ui.add_space(6.0);
            let pass = self.save_creds.get_or_insert_with(String::new);
            let field = ui.add(egui::TextEdit::singleline(pass).password(true).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 8.0)));
            field.request_focus();
            ui.add_space(24.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::button_sized(ui, None, "Zrušit", Btn::Secondary, true, 96.0).clicked() {
                    close = true;
                }
                if widgets::button_sized(ui, None, "Uložit", Btn::Primary, !pass.is_empty(), 96.0).clicked() {
                    ok = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !pass.is_empty() {
                ok = true;
            }
        });
        if ok {
            let pass = self.save_creds.take().unwrap_or_default();
            self.cfg.remember_login = true;
            self.cfg.save();
            self.send(Cmd::Login { user, pass, remember: true });
        } else if close || modal.should_close() {
            self.save_creds = None;
        }
    }

    fn ui_sidebar(&mut self, ctx: &egui::Context) {
        let pending = self
            .pending_keys()
            .iter()
            .filter(|k| self.row(k).is_none_or(|r| !self.finished_in_op(r)))
            .count();
        egui::SidePanel::left("nav")
            .exact_width(240.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(Frame::new().fill(SIDEBAR).inner_margin(Margin { left: 12, right: 12, top: 20, bottom: 16 }))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                let items = [
                    (View::Dashboard, icon::HOME, "Přehled", pending),
                    (View::Wizard, icon::PACKAGE, "Nová instalace", 0),
                    (View::History, icon::HISTORY, "Historie", 0),
                    (View::Settings, icon::SETTINGS, "Nastavení", 0),
                ];
                for (view, ic, label, count) in items {
                    if widgets::nav_item(ui, ic, label, self.view == view, count).clicked() {
                        self.actions.push(Action::Navigate(view));
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    let busy = self.busy();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
                    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
                    row.add_space(12.0);
                    row.label(widgets::icon_text(icon::USER, 16.0, SUBTLE));
                    row.add_space(4.0);
                    row.add(egui::Label::new(RichText::new(&self.user).size(14.0).color(TEXT)).truncate());
                    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let resp = ui
                            .add_enabled(!busy, egui::Button::new(RichText::new("Odhlásit").size(13.0).color(MUTED)).frame(false))
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            self.actions.push(Action::Ask(Confirm::Logout));
                        }
                    });
                    ui.add_space(8.0);
                    ui.separator();
                });
            });
    }

    /// Package names in queue order: finished, current, then the rest of the requested keys.
    fn queue_names(&self, op: &OpUi, p: &Progress) -> Vec<String> {
        let mut names = op.done.clone();
        if !names.contains(&p.package) {
            names.push(p.package.clone());
        }
        for k in &op.keys {
            if let Some(r) = self.row(k) {
                if !names.contains(&r.info.name) {
                    names.push(r.info.name.clone());
                }
            }
        }
        if names.len() == p.steps { names } else { Vec::new() }
    }

    pub fn ui_operation(&mut self, ui: &mut egui::Ui) {
        let Some(op) = self.op.clone() else { return };
        widgets::card(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::spinner(ui, 16.0);
                ui.add_space(4.0);
                let title = op.progress.as_ref().map(|p| p.package.clone()).unwrap_or_else(|| op.title.clone());
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(widgets::semibold_text(title, 15.0, TEXT));
                    let stage = op.progress.as_ref().map(|p| p.stage.clone()).unwrap_or_else(|| "Připravuji…".into());
                    ui.label(widgets::path_job(&stage, 13.0, MUTED));
                });
                let cancellable = op.progress.as_ref().is_none_or(|p| p.cancellable) || op.waiting_geostore;
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if cancellable {
                        let cancelling = self.cancel.load(Ordering::Relaxed);
                        let label = if cancelling { "Přerušuji…" } else { "Přerušit instalaci" };
                        if widgets::button(ui, None, label, Btn::Secondary, !cancelling).clicked() {
                            self.actions.push(Action::Ask(Confirm::CancelOp));
                        }
                    } else {
                        ui.label(RichText::new("Tento krok nelze přerušit").size(12.5).color(SUBTLE));
                    }
                });
            });
            ui.add_space(16.0);
            widgets::progress_bar(ui, op.progress.as_ref().and_then(|p| p.fraction));
            if let Some(p) = &op.progress {
                if !p.detail.is_empty() {
                    ui.add_space(8.0);
                    ui.label(widgets::numbers_job(&p.detail, 13.5, MUTED));
                }
                if p.steps > 1 {
                    ui.add_space(16.0);
                    let names = self.queue_names(&op, p);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
                    let gap = 4.0;
                    let w = (rect.width() - gap * (p.steps - 1) as f32) / p.steps as f32;
                    for i in 0..p.steps {
                        let x = rect.left() + i as f32 * (w + gap);
                        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, 4.0));
                        let (color, ink) = if i + 1 < p.step {
                            (SUCCESS, SUCCESS)
                        } else if i + 1 == p.step {
                            (ACCENT, ACCENT)
                        } else {
                            (TRACK, SUBTLE)
                        };
                        ui.painter().rect_filled(seg, CornerRadius::same(2), color);
                        if let Some(name) = names.get(i) {
                            let label = name.clone();
                            ui.painter().text(
                                egui::pos2(x, rect.bottom() + 8.0),
                                egui::Align2::LEFT_TOP,
                                label,
                                egui::FontId::new(12.5, egui::FontFamily::Proportional),
                                ink,
                            );
                        }
                    }
                    ui.add_space(26.0);
                }
            }
            if op.waiting_geostore {
                ui.add_space(12.0);
                widgets::banner(
                    ui,
                    widgets::Tone::Warn,
                    "Čekám na ukončení GeoStore V6",
                    "Uložte svou práci a zavřete všechna okna GeoStore V6. Instalace potom automaticky pokračuje.",
                    0.0,
                    |_| {},
                );
            }
        });
        ui.add_space(12.0);
    }

    pub fn ui_result(&mut self, ui: &mut egui::Ui) {
        let Some((ok, text)) = self.result.clone() else { return };
        let mut lines = text.lines();
        let head = lines.next().unwrap_or_default().trim_end_matches('.').to_string();
        let rest: Vec<&str> = lines.collect();
        let tone = if ok {
            widgets::Tone::Success
        } else if head.contains("zrušena") || head.contains("upozorněním") {
            widgets::Tone::Warn
        } else {
            widgets::Tone::Danger
        };
        widgets::banner(ui, tone, &head, &rest.join("\n"), 40.0, |ui| {
            if widgets::button(ui, Some(icon::CANCEL), "", Btn::Ghost, true).on_hover_text("Skrýt").clicked() {
                self.actions.push(Action::DismissResult);
            }
        });
    }

    pub fn status_text(row: &PackageRow) -> (&'static str, egui::Color32, Option<&'static str>) {
        let estimated = (row.status == Status::UpToDate && !row.from_state)
            .then_some("Stav je odhadnut podle data instalovaných souborů. Po první instalaci přes tuto aplikaci bude přesný.");
        match row.status {
            Status::UpToDate => ("Aktuální", SUCCESS, estimated),
            Status::UpdateAvailable => ("Nová verze", DANGER, None),
            Status::NotInstalled => ("Nenainstalováno", SUBTLE, None),
        }
    }

    pub fn open(path: impl Into<PathBuf>) {
        let p = path.into();
        let _ = std::fs::create_dir_all(if p.extension().is_some() { p.parent().unwrap_or(&p).to_path_buf() } else { p.clone() });
        local::open_in_explorer(&p);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.title_styled {
            self.title_styled = local::style_title_bar(crate::WINDOW_TITLE);
        }
        self.pump_events(ctx);
        self.tick(ctx);
        if !self.logged_in {
            self.ui_login(ctx);
        } else {
            self.ui_sidebar(ctx);
            egui::CentralPanel::default()
                .frame(Frame::new().fill(BG).inner_margin(Margin { left: 32, right: 32, top: 28, bottom: 20 }))
                .show(ctx, |ui| match self.view {
                    View::Dashboard => self.ui_dashboard(ui),
                    View::Wizard => self.ui_wizard(ui),
                    View::History => self.ui_history(ui),
                    View::Settings => self.ui_settings(ui),
                });
            self.ui_confirm(ctx);
            self.ui_save_creds(ctx);
        }
        self.resume_pending();
        self.apply_actions();
        if self.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
