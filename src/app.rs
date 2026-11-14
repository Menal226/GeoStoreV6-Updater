use crate::catalog::EDITOR;
use crate::config::{Config, State};
use crate::fmt;
use crate::logfile::{self, LogLine};
use crate::logic::{self, PackageRow, Status};
use crate::portal::Licence;
use crate::theme::{self, *};
use crate::widgets::{self, icon, Btn};
use crate::worker::{self, CleanPlan, Cmd, Event, InstallOptions, Outcome, Progress};
use crate::{creds, daemon, local, paths, scheduler, t};
use chrono::{DateTime, Local};
use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, RichText};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_LOG_LINES: usize = 6000;

#[derive(Default, Clone, Copy)]
pub struct Launch {
    pub relaunch: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Wizard,
    History,
    Settings,
    Help,
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
    pub update: bool,
    pub progress: Option<Progress>,
    pub waiting_geostore: bool,
    pub keys: Vec<String>,
    pub done: Vec<String>,
}

impl OpUi {
    pub fn new(title: impl Into<String>, update: bool, keys: Vec<String>) -> Self {
        Self { title: title.into(), update, progress: None, waiting_geostore: false, keys, done: Vec::new() }
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
    Uninstall,
}

/// Install request carried across the UAC restart, so the elevated copy picks up where the user left off.
#[derive(serde::Serialize, serde::Deserialize)]
enum Resume {
    UpdateAll(Vec<String>),
    Install(Vec<String>),
    Reinstall(String),
    InstallModule(String),
    CleanInstall(CleanPlan),
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
    ForgetCreds,
    SaveCreds,
    LicmanLogin,
    LicmanReset,
    Uninstall,
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
    pub result: Option<(Outcome, String)>,
    pub log: Vec<LogLine>,
    pub show_detail: bool,
    pub geostore_running: bool,
    last_poll: Instant,
    pub bg_busy: bool,
    pub last_auto_check: Option<DateTime<Local>>,
    pub wizard: WizardState,
    pub licences: Licences,
    pub licman_account: Option<String>,
    pub licman_login: Option<(String, String)>,
    pub confirm: Option<Confirm>,
    pub actions: Vec<Action>,
    pub task_error: Option<String>,
    pub creds_stored: bool,
    auto_login_started: bool,
    preview_confirm: bool,
    title_styled: bool,
    close_requested: bool,
    resume: Option<Resume>,
    pub save_creds: Option<String>,
    pub(crate) log_rev: u64,
    pub(crate) history: Option<crate::views::history::HistoryCache>,
    pub(crate) downloads_size: Option<(Instant, u64)>,
    pub(crate) settings_saved: Option<Instant>,
    shot_at: Option<Instant>,
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
        let (tx, rx, cancel) = worker::spawn(Some(cc.egui_ctx.clone()));
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
        if !cfg!(debug_assertions) || std::env::var_os("GSU_DAEMON").is_some() {
            task_error = scheduler::install().err().map(|e| worker::friendly_error(&e));
            daemon::ensure_running();
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
            bg_busy: worker::operation_running(),
            last_auto_check: State::load().last_auto_check,
            wizard: WizardState::default(),
            licences: Licences::Idle,
            licman_account: None,
            licman_login: None,
            confirm: None,
            actions: Vec::new(),
            task_error,
            creds_stored: stored.is_some(),
            auto_login_started: false,
            preview_confirm: false,
            title_styled: false,
            close_requested: false,
            resume: Resume::take(launch.relaunch),
            save_creds: None,
            log_rev: 0,
            history: None,
            downloads_size: None,
            settings_saved: None,
            shot_at: None,
            cfg,
        };
        if cfg!(debug_assertions) {
            app.apply_preview();
        }
        app
    }

    fn preview(option: &str) -> bool {
        cfg!(debug_assertions) && std::env::var("GSU_PREVIEW").is_ok_and(|m| m.split(',').any(|o| o.trim() == option))
    }

    fn preview_licences(list: &mut Vec<Licence>) {
        if Self::preview("anon") {
            for l in list.iter_mut() {
                l.company = "Geodetická kancelář s.r.o.".into();
                l.project = "GSV6 - Geodetická kancelář".into();
            }
        }
        let Some(first) = list.first().cloned().filter(|_| Self::preview("licences")) else { return };
        for (i, (project, valid, trial)) in [("GSV6 - Pobočka Zlín", "31.12.2027", false), ("GSV6 - zkušební", "30.11.2026", true)].into_iter().enumerate() {
            let mut l = first.clone();
            l.guid = format!("{}-preview{i}", l.guid);
            l.project = project.into();
            l.valid_to = valid.into();
            l.trial = trial;
            list.push(l);
        }
    }

    /// Debug-only screenshot aid. GSU_PREVIEW is a comma list: admin (install controls on), op (sample progress card), confirm (open the update dialog), licences (two extra fake licences), anon (sample user and company names), login (stay on the sign-in screen), view=wizard|history|settings|help.
    fn apply_preview(&mut self) {
        let Ok(mode) = std::env::var("GSU_PREVIEW") else { return };
        self.elevated = true;
        let opts: Vec<&str> = mode.split(',').map(str::trim).collect();
        self.preview_confirm = opts.contains(&"confirm");
        if opts.contains(&"demo") {
            self.auto_login_started = true;
            self.send(Cmd::Login { user: "test".into(), pass: "test".into(), remember: false });
        }
        if opts.contains(&"login") {
            self.auto_login_started = true;
            self.login.user = "jnovak".into();
            self.login.pass = "heslo1234".into();
        }
        for o in &opts {
            match o.strip_prefix("view=") {
                Some("wizard") => self.view = View::Wizard,
                Some("history") => self.view = View::History,
                Some("settings") => self.view = View::Settings,
                Some("help") => self.view = View::Help,
                _ => {}
            }
        }
        if opts.contains(&"op") {
            let mut op = OpUi::new(t!("app.op_update"), true, vec!["DTMCR".into(), "IG".into()]);
            op.done.push("V6-DTMČR".into());
            op.progress = Some(Progress {
                kind: worker::StageKind::Download,
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

    pub(crate) fn set_language(&mut self, lang: crate::i18n::Lang) {
        self.cfg.language = Some(lang);
        self.cfg.save();
        crate::i18n::set(lang);
        self.history = None;
        for row in &mut self.rows {
            row.info = crate::catalog::module_info(&row.key);
        }
    }

    pub fn busy(&self) -> bool {
        self.op.is_some()
    }

    pub fn can_install(&self) -> bool {
        self.op.is_none() && !self.bg_busy
    }

    fn admin_request(action: &Action) -> Option<Resume> {
        match action {
            Action::Ask(Confirm::UpdateAll(k)) => Some(Resume::UpdateAll(k.clone())),
            Action::Ask(Confirm::Reinstall(k)) => Some(Resume::Reinstall(k.clone())),
            Action::Ask(Confirm::InstallModule(k)) => Some(Resume::InstallModule(k.clone())),
            Action::Ask(Confirm::CleanInstall(p)) => Some(Resume::CleanInstall(p.clone())),
            Action::Install(k, _) => Some(Resume::Install(k.clone())),
            Action::CleanInstall(p) => Some(Resume::CleanInstall(p.clone())),
            _ => None,
        }
    }

    /// Restarts through the UAC prompt and hands the request to the elevated copy.
    fn elevate_for(&mut self, resume: Resume) {
        self.cfg.save();
        resume.save();
        if local::relaunch_elevated() {
            self.close_requested = true;
            return;
        }
        let _ = std::fs::remove_file(Resume::path());
        self.result = Some((Outcome::Failed, t!("app.admin_denied").into()));
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

    fn pump_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::LoggedIn { user } => {
                    self.logged_in = true;
                    self.user = if Self::preview("anon") { "jnovak".into() } else { user };
                    self.login.busy = false;
                    self.login.error = None;
                    self.login.pass.clear();
                    self.creds_stored = creds::exists() || Self::preview("anon");
                }
                Event::LoginFailed(e) => {
                    self.login.busy = false;
                    if self.logged_in {
                        self.result = Some((Outcome::Failed, t!("app.save_creds_failed", error = e)));
                    } else {
                        self.login.error = Some(e);
                    }
                }
                Event::LoggedOut => {
                    self.logged_in = false;
                    self.licences = Licences::Idle;
                    self.licman_account = None;
                    self.wizard = WizardState::default();
                    self.rows.clear();
                    self.last_check = None;
                    self.check_error = None;
                    self.result = None;
                    self.creds_stored = false;
                }
                Event::Checking => self.checking = true,
                Event::Overview { rows, checked_at } => {
                    self.rows = rows;
                    self.last_check = checked_at;
                    self.checking = false;
                    self.check_error = None;
                    self.on_overview();
                }
                Event::CheckFailed(e) => {
                    self.checking = false;
                    self.check_error = Some(e);
                }
                Event::OpStarted { title, update } => {
                    let keys = self.op.take().map(|o| o.keys).unwrap_or_default();
                    self.op = Some(OpUi::new(title, update, keys));
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
                    self.log_rev += 1;
                    self.log.push(line);
                    if self.log.len() > MAX_LOG_LINES {
                        let excess = self.log.len() - MAX_LOG_LINES;
                        self.log.drain(..excess);
                    }
                }
                Event::Licences { result, account } => {
                    self.licman_account = account;
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
                Event::OpFinished { outcome, summary } => {
                    self.op = None;
                    self.result = Some((outcome, summary));
                    self.cancel.store(false, Ordering::Relaxed);
                    if self.confirm == Some(Confirm::CancelOp) {
                        self.confirm = None;
                    }
                }
            }
        }
    }

    fn on_overview(&mut self) {
        if std::mem::take(&mut self.preview_confirm) {
            let pending = self.pending_keys();
            if !pending.is_empty() {
                self.confirm = Some(Confirm::UpdateAll(pending));
            }
        }
    }

    /// The tray process checks and installs on its own; when it has, the window reloads the overview.
    fn poll_background(&mut self) {
        let last = State::load().last_auto_check;
        let checked = last != self.last_auto_check;
        self.last_auto_check = last;
        let busy = self.op.is_none() && worker::operation_running();
        let finished = self.bg_busy && !busy;
        self.bg_busy = busy;
        if (checked || finished) && !busy && self.logged_in && !self.checking && self.op.is_none() {
            self.checking = true;
            self.send(Cmd::Check);
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
            self.poll_background();
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn start_install(&mut self, keys: Vec<String>, force: bool) {
        if !self.can_install() {
            return;
        }
        let opts = self.install_options(force);
        self.op = Some(OpUi::new(t!("app.op_update"), true, keys.clone()));
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
                        self.login.error = Some(t!("app.fill_credentials").into());
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
                    self.op = Some(OpUi::new(t!("app.op_clean"), false, keys));
                    self.result = None;
                    self.send(Cmd::CleanInstall { plan, opts });
                }
                Action::MarkCurrent(key) => self.send(Cmd::MarkCurrent(key)),
                Action::SetTracked(key, on) => {
                    self.cfg.tracked.insert(key, on);
                    self.cfg.save();
                }
                Action::Cancel => {
                    if self.op.is_some() {
                        self.cancel.store(true, Ordering::Relaxed);
                    }
                }
                Action::Ask(c) => self.confirm = Some(c),
                Action::Navigate(v) => self.view = v,
                Action::DismissResult => self.result = None,
                Action::ForgetCreds => {
                    creds::delete();
                    self.creds_stored = false;
                }
                Action::SaveCreds => self.save_creds = Some(String::new()),
                Action::LicmanLogin => self.licman_login = Some((self.licman_account.clone().unwrap_or_default(), String::new())),
                Action::LicmanReset => {
                    self.licences = Licences::Loading;
                    self.send(Cmd::LicmanReset);
                }
                Action::Uninstall => self.uninstall(),
            }
        }
    }

    fn uninstall(&mut self) {
        // Installed through the setup, its own uninstaller removes the files and the Apps & features entry.
        let setup_uninstaller = crate::selfupdate::app_exe().and_then(|exe| exe.parent().map(|dir| dir.join("unins000.exe"))).filter(|p| p.exists());
        if let Some(unins) = setup_uninstaller {
            if std::process::Command::new(unins).arg("/SILENT").spawn().is_ok() {
                self.close_requested = true;
                return;
            }
        }
        daemon::stop();
        if let Err(e) = scheduler::uninstall() {
            self.view = View::Dashboard;
            self.result = Some((Outcome::Failed, t!("app.uninstall_failed", error = worker::friendly_error(&e))));
            return;
        }
        creds::delete();
        creds::delete_licman();
        if let Some(exe) = crate::selfupdate::app_exe() {
            let own_dir: Vec<PathBuf> = exe
                .parent()
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("geostore updater")))
                .map(|p| p.to_path_buf())
                .into_iter()
                .collect();
            let old = exe.with_extension("old.exe");
            local::remove_after_exit(&[exe, old], &[paths::data_dir()], &own_dir);
        }
        self.close_requested = true;
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
            Confirm::Uninstall => self.actions.push(Action::Uninstall),
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
            t!("app.geostore_running"),
            t!("app.geostore_running_body"),
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
                    t!("app.update_one_title", name = self.names(keys))
                } else {
                    t!("app.update_many_title", count = keys.len(), packages = fmt::packages(keys.len()))
                };
                (title, String::new(), t!("app.update"), self.geostore_warning())
            }
            Confirm::Reinstall(key) => (
                t!("app.reinstall_title", name = self.names(std::slice::from_ref(key))),
                String::new(),
                t!("app.reinstall"),
                self.geostore_warning(),
            ),
            Confirm::InstallModule(key) => (
                t!("app.install_title", name = self.names(std::slice::from_ref(key))),
                String::new(),
                t!("app.install"),
                Some((t!("app.env_overwrite"), t!("app.env_overwrite_body"))),
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
                    lines.push((t!("app.licence").to_string(), name.unwrap_or_else(|| t!("app.licence_fallback").into())));
                }
                (
                    t!("app.clean_title").into(),
                    String::new(),
                    t!("app.start_install"),
                    Some((t!("app.config_overwrite"), t!("app.config_overwrite_body"))),
                )
            }
            Confirm::CancelOp => (
                t!("app.cancel_title").into(),
                t!("app.cancel_body").into(),
                t!("app.cancel_install"),
                None,
            ),
            Confirm::Uninstall => (
                t!("app.uninstall_title").into(),
                t!("app.uninstall_body").into(),
                t!("app.uninstall"),
                None,
            ),
            Confirm::Logout => (
                t!("app.logout_title").into(),
                t!("app.logout_body").into(),
                t!("app.logout"),
                None,
            ),
        };
        let mut close = false;
        let mut ok = false;
        let modal = egui::Modal::new(egui::Id::new("confirm")).frame(Self::modal_frame()).backdrop_color(egui::Color32::from_black_alpha(100)).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(widgets::semibold_text(&title, 18.0, TEXT));
            ui.add_space(16.0);
            if !body.is_empty() {
                ui.add(egui::Label::new(RichText::new(&body).size(14.0).color(MUTED)).wrap());
                ui.add_space(if lines.is_empty() { 16.0 } else { 8.0 });
            }
            if !lines.is_empty() {
                widgets::change_list(ui, &lines);
                ui.add_space(16.0);
            }
            if let Some((head, text)) = warn {
                widgets::banner(ui, widgets::Tone::Warn, head, text, 0.0, |_| {});
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let kind = if matches!(c, Confirm::Logout | Confirm::CancelOp | Confirm::Uninstall) { Btn::Danger } else { Btn::Primary };
                let back = if c == Confirm::CancelOp { t!("app.continue") } else { t!("app.cancel") };
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
        let modal = egui::Modal::new(egui::Id::new("save_creds")).frame(Self::modal_frame()).backdrop_color(egui::Color32::from_black_alpha(100)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(widgets::semibold_text(t!("app.save_creds_title"), 18.0, TEXT));
            ui.add_space(16.0);
            ui.add(
                egui::Label::new(
                    RichText::new(t!("app.save_creds_body", user = user))
                        .size(14.0)
                        .color(MUTED),
                )
                .wrap(),
            );
            ui.add_space(16.0);
            ui.label(RichText::new(t!("app.password")).size(13.0).color(MUTED));
            ui.add_space(6.0);
            let pass = self.save_creds.get_or_insert_with(String::new);
            let field = ui.add(egui::TextEdit::singleline(pass).password(true).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 8.0)));
            field.request_focus();
            ui.add_space(24.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::button_sized(ui, None, t!("app.cancel"), Btn::Secondary, true, 96.0).clicked() {
                    close = true;
                }
                if widgets::button_sized(ui, None, t!("app.save"), Btn::Primary, !pass.is_empty(), 96.0).clicked() {
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
            // `self.user` is the name the portal reports back, which need not be the one it accepts at sign-in.
            let typed = self.login.user.trim();
            let user = if typed.is_empty() { user } else { typed.to_string() };
            self.send(Cmd::Login { user, pass, remember: true });
        } else if close || modal.should_close() {
            self.save_creds = None;
        }
    }

    fn ui_licman_login(&mut self, ctx: &egui::Context) {
        let Some((user, pass)) = self.licman_login.as_mut() else { return };
        let mut ok = false;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("licman_login")).frame(Self::modal_frame()).backdrop_color(egui::Color32::from_black_alpha(100)).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(widgets::semibold_text(t!("app.licman_title"), 18.0, TEXT));
            ui.add_space(16.0);
            ui.label(RichText::new(t!("app.username")).size(13.0).color(MUTED));
            ui.add_space(6.0);
            let user_field = ui.add(egui::TextEdit::singleline(user).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 8.0)));
            ui.add_space(12.0);
            ui.label(RichText::new(t!("app.password")).size(13.0).color(MUTED));
            ui.add_space(6.0);
            let pass_field = ui.add(egui::TextEdit::singleline(pass).password(true).desired_width(f32::INFINITY).margin(egui::vec2(12.0, 8.0)));
            if !user_field.has_focus() && !pass_field.has_focus() {
                if user.is_empty() { user_field.request_focus() } else { pass_field.request_focus() }
            }
            ui.add_space(24.0);
            let filled = !user.trim().is_empty() && !pass.is_empty();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::button_sized(ui, None, t!("app.cancel"), Btn::Secondary, true, 96.0).clicked() {
                    close = true;
                }
                if widgets::button_sized(ui, None, t!("app.sign_in"), Btn::Primary, filled, 96.0).clicked() {
                    ok = true;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && filled {
                ok = true;
            }
        });
        if ok {
            if let Some((user, pass)) = self.licman_login.take() {
                self.licences = Licences::Loading;
                self.send(Cmd::LicmanLogin { user: user.trim().to_string(), pass });
            }
        } else if close || modal.should_close() {
            self.licman_login = None;
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
                    (View::Dashboard, icon::HOME, t!("app.nav_dashboard"), pending),
                    (View::History, icon::HISTORY, t!("app.nav_history"), 0),
                    (View::Settings, icon::SETTINGS, t!("app.nav_settings"), 0),
                    (View::Help, icon::HELP, t!("app.nav_help"), 0),
                    (View::Wizard, icon::PACKAGE, t!("app.nav_wizard"), 0),
                ];
                for (view, ic, label, count) in items {
                    // The clean installation is a rare, disruptive flow, set apart from the everyday pages.
                    if view == View::Wizard {
                        ui.add_space(6.0);
                        let y = ui.cursor().top();
                        ui.painter().hline(ui.max_rect().x_range().shrink(4.0), y, egui::Stroke::new(1.0_f32, BORDER));
                        ui.add_space(8.0);
                    }
                    if widgets::nav_item(ui, ic, label, self.view == view, count).clicked() {
                        self.actions.push(Action::Navigate(view));
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    let busy = self.busy();
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::hover());
                    let logout_w = widgets::button_width(ui, Some(icon::SIGN_OUT), t!("app.logout"));
                    // The logout button's left padding is empty, so the name may run up to 8px before its icon.
                    widgets::icon_label(ui, rect, icon::USER, &self.user, false, rect.width() - 40.0 - (logout_w - 16.0) - 2.0 - 8.0);
                    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::right_to_left(Align::Center)));
                    row.add_space(2.0);
                    if widgets::button_ex(&mut row, Some(icon::SIGN_OUT), t!("app.logout"), Btn::Ghost, !busy, 0.0, 32.0).clicked() {
                        self.actions.push(Action::Ask(Confirm::Logout));
                    }
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
                let stage = op.progress.as_ref().map(|p| p.stage.clone()).unwrap_or_else(|| t!("app.preparing").into());
                let mut short = widgets::without_paths(&stage);
                if let Some(p) = op.progress.as_ref().filter(|p| p.steps > 1) {
                    short = format!("{} · {}", short.trim_end_matches('…'), t!("app.package_step", step = p.step, steps = p.steps));
                }
                let cancellable = op.progress.as_ref().is_none_or(|p| p.cancellable) || op.waiting_geostore;
                let cancelling = self.cancel.load(Ordering::Relaxed);
                let label = match (cancelling, op.update) {
                    (true, _) => t!("app.cancelling"),
                    (false, true) => t!("app.cancel_update"),
                    (false, false) => t!("app.cancel_install"),
                };
                let right_w = if cancellable {
                    widgets::button_width(ui, Some(icon::CANCEL), label)
                } else {
                    ui.fonts(|f| f.layout_no_wrap(t!("app.not_cancellable").to_string(), egui::FontId::proportional(13.0), SUBTLE).size().x)
                };
                ui.label(widgets::semibold_text(title, 15.0, TEXT));
                let text_w = (ui.available_width() - right_w - 16.0).max(40.0);
                ui.allocate_ui_with_layout(egui::vec2(text_w, 32.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(format!("· {short}")).size(13.5).color(MUTED)).truncate());
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if cancellable {
                        if widgets::button(ui, Some(icon::CANCEL), label, Btn::Secondary, !cancelling).clicked() {
                            self.actions.push(Action::Ask(Confirm::CancelOp));
                        }
                    } else {
                        ui.label(RichText::new(t!("app.not_cancellable")).size(13.0).color(SUBTLE));
                    }
                });
            });
            ui.add_space(8.0);
            widgets::progress_bar(ui, op.progress.as_ref().and_then(|p| p.fraction));
            if let Some(p) = &op.progress {
                let percent = p.fraction.map(|f| format!("{:.0} %", f * 100.0));
                let detail = match (percent, p.detail.is_empty()) {
                    (Some(pc), true) => pc,
                    (Some(pc), false) => format!("{pc} · {}", p.detail),
                    (None, _) => p.detail.clone(),
                };
                if !detail.is_empty() {
                    ui.add_space(4.0);
                    ui.label(widgets::numbers_job(&detail, 13.5, MUTED));
                }
                let names = self.queue_names(&op, p);
                // The dashboard table already shows each package's state; elsewhere these chips are the only queue view.
                if p.steps > 1 && !names.is_empty() && self.view != View::Dashboard {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (i, name) in names.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(10.0);
                            }
                            let (glyph, color, text) = if i + 1 < p.step {
                                (Some(icon::CHECK), SUCCESS, RichText::new(name).size(13.0).color(MUTED))
                            } else if i + 1 == p.step {
                                (None, ACCENT, widgets::semibold_text(name, 13.0, TEXT))
                            } else {
                                (None, SUBTLE, RichText::new(name).size(13.0).color(SUBTLE))
                            };
                            match glyph {
                                Some(g) => {
                                    ui.label(widgets::icon_text(g, 12.0, color));
                                }
                                None => {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), egui::Sense::hover());
                                    widgets::paint_mark(ui, r.center(), if i + 1 == p.step { widgets::Mark::Pending } else { widgets::Mark::Empty });
                                }
                            }
                            ui.label(text);
                        }
                    });
                }
            }
            if op.waiting_geostore {
                ui.add_space(12.0);
                widgets::banner(
                    ui,
                    widgets::Tone::Warn,
                    t!("app.waiting_geostore"),
                    t!("app.waiting_geostore_body"),
                    0.0,
                    |_| {},
                );
            }
        });
        ui.add_space(12.0);
    }

    pub fn ui_result(&mut self, ui: &mut egui::Ui) {
        let Some((outcome, text)) = self.result.clone() else { return };
        let mut lines = text.lines();
        let head = lines.next().unwrap_or_default().trim_end_matches('.').to_string();
        let rest: Vec<&str> = lines.collect();
        let tone = match outcome {
            Outcome::Success => widgets::Tone::Success,
            Outcome::Warnings | Outcome::Cancelled => widgets::Tone::Warn,
            Outcome::Failed => widgets::Tone::Danger,
        };
        widgets::banner(ui, tone, &head, &rest.join("\n"), 40.0, |ui| {
            if widgets::button(ui, Some(icon::CANCEL), "", Btn::Ghost, true).on_hover_text(t!("app.hide")).clicked() {
                self.actions.push(Action::DismissResult);
            }
        });
    }

    pub fn status_text(row: &PackageRow) -> (&'static str, egui::Color32, Option<&'static str>) {
        let estimated = (row.status == Status::UpToDate && !row.from_state)
            .then_some(t!("app.status_estimated"));
        match row.status {
            Status::UpToDate => (t!("app.status_current"), SUCCESS, estimated),
            Status::UpdateAvailable => (t!("app.status_new"), ACCENT, None),
            Status::NotInstalled => (t!("app.status_missing"), SUBTLE, None),
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
        self.pump_events();
        self.tick(ctx);
        if !self.logged_in {
            self.ui_login(ctx);
        } else {
            self.ui_sidebar(ctx);
            egui::CentralPanel::default()
                .frame(Frame::new().fill(BG).inner_margin(Margin { left: 32, right: 20, top: 28, bottom: if self.view == View::Wizard { 0 } else { 20 } }))
                .show(ctx, |ui| match self.view {
                    View::Dashboard => self.ui_dashboard(ui),
                    View::Wizard => self.ui_wizard(ui),
                    View::History => self.ui_history(ui),
                    View::Settings => self.ui_settings(ui),
                    View::Help => self.ui_help(ui),
                });
            self.ui_confirm(ctx);
            self.ui_save_creds(ctx);
            self.ui_licman_login(ctx);
        }
        self.resume_pending();
        self.apply_actions();
        if self.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if cfg!(debug_assertions) {
            self.debug_shot(ctx);
        }
    }
}

/// Debug-only: GSU_SHOT_ZOOM renders screenshots at a larger scale (e.g. 1.5 for the README images).
pub fn shot_zoom() -> f32 {
    if !cfg!(debug_assertions) || std::env::var_os("GSU_SHOT").is_none() {
        return 1.0;
    }
    std::env::var("GSU_SHOT_ZOOM").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0)
}

impl App {
    /// Debug-only: with GSU_SHOT set to a .bmp path, saves the window there a few seconds after start and exits.
    fn debug_shot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("GSU_SHOT") else { return };
        let started = *self.shot_at.get_or_insert_with(Instant::now);
        ctx.set_zoom_factor(shot_zoom());
        ctx.request_repaint_after(Duration::from_millis(100));
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let mut out = Vec::with_capacity(54 + w * h * 4);
            out.extend_from_slice(b"BM");
            out.extend_from_slice(&((54 + w * h * 4) as u32).to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0]);
            out.extend_from_slice(&(w as i32).to_le_bytes());
            out.extend_from_slice(&(-(h as i32)).to_le_bytes());
            out.extend_from_slice(&[1, 0, 32, 0]);
            out.extend_from_slice(&[0u8; 24]);
            for p in &image.pixels {
                out.extend_from_slice(&[p.b(), p.g(), p.r(), 255]);
            }
            let _ = std::fs::write(&path, out);
            std::process::exit(0);
        }
        let wait = std::env::var("GSU_SHOT_WAIT").ok().and_then(|s| s.parse().ok()).unwrap_or(4000);
        if started.elapsed() > Duration::from_millis(wait) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }
}
