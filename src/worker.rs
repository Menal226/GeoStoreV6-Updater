use crate::catalog::EDITOR;
use crate::config::{self, InstalledRecord, State};
use crate::fmt;
use crate::i18n;
use crate::installer;
use crate::local::{self, DavkaLog};
use crate::logfile::{self, Level, LogLine};
use crate::logic::{self, PackageRow};
use crate::paths;
use crate::portal::{self, Cancelled, Licence, Portal, RemoteFile};
use crate::t;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Local};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub backup: bool,
    pub keep_downloads: bool,
    pub tracked: Vec<String>,
    pub force: bool,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CleanPlan {
    pub editor: bool,
    pub modules: Vec<String>,
    pub hasp: bool,
    pub vcredist: bool,
    #[serde(default)]
    pub licence: Option<String>,
}

impl CleanPlan {
    pub fn is_empty(&self) -> bool {
        !self.editor && self.modules.is_empty() && self.licence.is_none()
    }
}

pub enum Cmd {
    Login { user: String, pass: String, remember: bool },
    Logout,
    Check,
    Install { keys: Vec<String>, opts: InstallOptions },
    CleanInstall { plan: CleanPlan, opts: InstallOptions },
    MarkCurrent(String),
    Licences,
    LicmanLogin { user: String, pass: String },
    LicmanReset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StageKind {
    Download,
    Extract,
    #[default]
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success,
    Warnings,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub kind: StageKind,
    pub step: usize,
    pub steps: usize,
    pub package: String,
    pub stage: String,
    pub fraction: Option<f32>,
    pub detail: String,
    pub cancellable: bool,
}

pub enum Event {
    LoggedIn { user: String },
    LoginFailed(String),
    LoggedOut,
    Checking,
    Overview { rows: Vec<PackageRow>, checked_at: Option<DateTime<Local>> },
    CheckFailed(String),
    OpStarted { title: String, update: bool },
    Progress(Progress),
    WaitingForGeoStore(bool),
    Log(LogLine),
    OpFinished { outcome: Outcome, summary: String },
    /// `account` is the separate licence-server account in use, None when the portal account is used.
    Licences { result: Result<Vec<Licence>, String>, account: Option<String> },
}

#[derive(Clone)]
pub struct Emitter {
    tx: Sender<Event>,
    ctx: Option<eframe::egui::Context>,
}

impl Emitter {
    pub fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint();
        }
    }

    pub fn log(&self, level: Level, text: impl Into<String>) {
        let line = LogLine {
            time: Local::now(),
            level,
            text: text.into(),
        };
        logfile::append(&line);
        self.send(Event::Log(line));
    }

    pub fn fail(&self, prefix: &str, e: &anyhow::Error) -> String {
        let msg = friendly_error(e);
        self.log(Level::Error, format!("{prefix}: {msg}"));
        let full = format!("{e:#}");
        if full != msg {
            self.log(Level::Detail, full);
        }
        msg
    }
}

pub fn spawn(ctx: Option<eframe::egui::Context>) -> (Sender<Cmd>, Receiver<Event>, Arc<AtomicBool>) {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    std::thread::Builder::new()
        .name("worker".into())
        .spawn(move || {
            let mut w = Worker {
                em: Emitter { tx: ev_tx, ctx },
                cancel: worker_cancel,
                portal: None,
                files: Vec::new(),
                state: State::load(),
            };
            while let Ok(cmd) = cmd_rx.recv() {
                w.handle(cmd);
            }
        })
        .expect("worker thread");
    (cmd_tx, ev_rx, cancel)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Editor { force: bool },
    Module { complete: bool },
}

#[derive(Clone)]
struct Job {
    key: String,
    name: String,
    file: RemoteFile,
    kind: JobKind,
}

#[derive(Default)]
struct Report {
    lines: Vec<String>,
    warnings: usize,
}

struct Extras {
    hasp: bool,
    vcredist: bool,
    licence: Option<String>,
}

struct Worker {
    em: Emitter,
    cancel: Arc<AtomicBool>,
    portal: Option<Portal>,
    files: Vec<RemoteFile>,
    state: State,
}

/// Held while packages are being installed, so the window and the tray process never install at the same time.
fn exclusive(name: &str) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(paths::data_dir().join(name))
}

fn operation_lock() -> Option<std::fs::File> {
    exclusive("operation.lock").ok()
}

/// The portal keeps one session per account, so the window and the tray process take turns instead of ending each other's.
fn portal_lock() -> Result<Option<std::fs::File>> {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match exclusive("portal.lock") {
            Ok(f) => return Ok(Some(f)),
            Err(e) if e.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
                if Instant::now() >= deadline {
                    bail!(t!("worker.portal_busy"));
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => return Ok(None),
        }
    }
}

pub fn operation_running() -> bool {
    operation_lock().is_none()
}

pub fn friendly_error(e: &anyhow::Error) -> String {
    for cause in e.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_connect() || re.is_timeout() {
                return t!("worker.no_connection").into();
            }
        }
    }
    let mut parts: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    parts.dedup();
    parts.join(" – ")
}

impl Worker {
    fn handle(&mut self, cmd: Cmd) {
        self.state = State::load();
        match cmd {
            Cmd::Login { user, pass, remember } => self.login(user, pass, remember),
            Cmd::Logout => {
                self.portal = None;
                self.files.clear();
                crate::creds::delete();
                crate::creds::delete_licman();
                self.em.log(Level::Info, t!("worker.logged_out"));
                self.em.send(Event::LoggedOut);
            }
            Cmd::Check => self.check(),
            Cmd::Install { keys, opts } => {
                let jobs = self.update_jobs(&keys, &opts);
                self.run_operation(true, jobs, opts, Extras { hasp: false, vcredist: false, licence: None });
            }
            Cmd::CleanInstall { plan, opts } => {
                let jobs = self.clean_jobs(&plan);
                let extras = Extras { hasp: plan.hasp, vcredist: plan.vcredist, licence: plan.licence.clone() };
                self.run_operation(false, jobs, opts, extras);
            }
            Cmd::Licences => self.send_licences(),
            Cmd::LicmanLogin { user, pass } => {
                let result = Portal::licman().and_then(|mut l| {
                    l.login(&user, &pass).context(t!("worker.licman_login_failed_account", user = user))?;
                    l.list_licences()
                });
                match result {
                    Ok(list) => {
                        if let Err(e) = crate::creds::save_licman(&user, &pass) {
                            self.em.log(Level::Warn, t!("worker.licman_save_failed", error = e));
                        }
                        self.em.log(Level::Info, t!("worker.licman_account", user = user));
                        self.em.send(Event::Licences { result: Ok(list), account: Some(user) });
                    }
                    Err(e) => {
                        self.em.log(Level::Warn, t!("worker.licences_failed", error = friendly_error(&e)));
                        let account = crate::creds::load_licman().map(|(u, _)| u);
                        self.em.send(Event::Licences { result: Err(friendly_error(&e)), account });
                    }
                }
            }
            Cmd::LicmanReset => {
                crate::creds::delete_licman();
                self.send_licences();
            }
            Cmd::MarkCurrent(key) => {
                if let Some(row) = self.rows().into_iter().find(|r| r.key == key) {
                    if let Some(f) = &row.latest {
                        self.record(&key, f, row.local_version.clone(), false);
                        self.em.log(
                            Level::Info,
                            t!("worker.marked_current", name = row.info.name, date = fmt::date(f.date)),
                        );
                    }
                }
                self.emit_overview();
            }
        }
    }

    fn login(&mut self, user: String, pass: String, remember: bool) {
        let result = portal_lock().and_then(|_lock| Portal::new().and_then(|mut p| p.login(&user, &pass).map(|_| p)));
        match result {
            Ok(p) => {
                if remember {
                    if let Err(e) = crate::creds::save(&user, &pass) {
                        self.em.log(Level::Warn, t!("worker.creds_save_failed", error = e));
                    }
                } else {
                    crate::creds::delete();
                }
                let name = p.user_name.clone();
                let roles: Vec<String> = p.roles.iter().map(|r| r.name.clone()).collect();
                self.portal = Some(p);
                self.em.log(
                    Level::Detail,
                    t!("log.logged_in", name = name, roles = roles.join(", ")),
                );
                self.em.send(Event::LoggedIn { user: name });
                self.check();
            }
            Err(e) => {
                let msg = self.em.fail(t!("worker.login_failed"), &e);
                self.em.send(Event::LoginFailed(msg));
            }
        }
    }

    fn check(&mut self) {
        self.em.send(Event::Checking);
        match self.with_portal(|p, _| p.list_files()) {
            Ok(files) => {
                self.files = files;
                self.state = State::update(|s| s.last_check = Some(Local::now()));
                let rows = self.rows();
                let pending = rows
                    .iter()
                    .filter(|r| r.status == logic::Status::UpdateAvailable)
                    .count();
                let outcome = match pending {
                    0 => t!("log.all_up_to_date").to_string(),
                    n => i18n::fill(i18n::plural("log.updates_available", n as u64), &[("n", n.to_string())]),
                };
                self.em.log(
                    Level::Info,
                    t!("log.check_done", outcome = outcome, count = rows.len(), packages = fmt::packages(rows.len())),
                );
                self.emit_overview();
            }
            Err(e) => {
                let msg = self.em.fail(t!("worker.check_failed"), &e);
                self.em.send(Event::CheckFailed(msg));
            }
        }
    }

    fn rows(&self) -> Vec<PackageRow> {
        logic::compute_rows(&self.files, &self.state)
    }

    fn emit_overview(&self) {
        self.em.send(Event::Overview {
            rows: self.rows(),
            checked_at: self.state.last_check,
        });
    }

    fn with_portal<T>(&mut self, f: impl FnOnce(&mut Portal, &Emitter) -> Result<T>) -> Result<T> {
        let mut portal = self
            .portal
            .take()
            .ok_or_else(|| anyhow!(t!("worker.not_logged_in")))?;
        let result = portal_lock().and_then(|_lock| {
            portal.ensure_session()?;
            f(&mut portal, &self.em)
        });
        self.portal = Some(portal);
        result
    }

    fn update_jobs(&self, keys: &[String], opts: &InstallOptions) -> Vec<Job> {
        let rows = self.rows();
        let mut jobs: Vec<Job> = keys
            .iter()
            .filter_map(|key| {
                let row = rows.iter().find(|r| &r.key == key)?;
                let file = row.latest.clone()?;
                Some(Job {
                    key: key.clone(),
                    name: row.info.name.clone(),
                    file,
                    kind: if row.is_editor() {
                        JobKind::Editor { force: opts.force }
                    } else {
                        JobKind::Module { complete: false }
                    },
                })
            })
            .collect();
        sort_jobs(&mut jobs);
        jobs
    }

    fn clean_jobs(&self, plan: &CleanPlan) -> Vec<Job> {
        let rows = self.rows();
        let mut jobs = Vec::new();
        if plan.editor {
            if let Some(row) = rows.iter().find(|r| r.is_editor()) {
                if let Some(f) = row.latest.clone() {
                    jobs.push(Job {
                        key: EDITOR.into(),
                        name: row.info.name.clone(),
                        file: f,
                        kind: JobKind::Editor { force: true },
                    });
                }
            }
        }
        for key in &plan.modules {
            let Some(row) = rows.iter().find(|r| &r.key == key) else { continue };
            let Some(file) = row.complete.clone().or_else(|| row.latest.clone()) else { continue };
            let complete = row.latest.as_ref().map_or(true, |l| l.id != file.id);
            jobs.push(Job {
                key: key.clone(),
                name: row.info.name.clone(),
                file,
                kind: JobKind::Module { complete },
            });
        }
        sort_jobs(&mut jobs);
        jobs
    }

    fn run_operation(&mut self, update: bool, jobs: Vec<Job>, opts: InstallOptions, extras: Extras) {
        let title = if update { t!("worker.title_update") } else { t!("worker.title_clean") };
        self.cancel.store(false, Ordering::Relaxed);
        self.em.send(Event::OpStarted { title: title.to_string(), update });
        self.em.log(Level::Info, t!("worker.op_started", title = title));
        let result = if jobs.is_empty() && extras.licence.is_none() {
            Err(anyhow!(t!("worker.nothing_selected")))
        } else {
            match operation_lock() {
                Some(_lock) => self.run_jobs(jobs, &opts, extras),
                None => Err(anyhow!(t!("worker.other_running"))),
            }
        };
        self.em.send(Event::WaitingForGeoStore(false));
        let (outcome, summary) = match result {
            Ok(report) => {
                let ok = report.warnings == 0;
                let head = if ok {
                    t!("worker.install_ok")
                } else {
                    t!("worker.install_warnings")
                };
                let mut text = head.to_string();
                for l in &report.lines {
                    text.push('\n');
                    text.push_str(l);
                }
                self.em.log(if ok { Level::Success } else { Level::Warn }, text.clone());
                (if ok { Outcome::Success } else { Outcome::Warnings }, text)
            }
            Err(e) if portal::is_cancelled(&e) => {
                self.em.log(Level::Warn, t!("worker.cancelled"));
                (Outcome::Cancelled, t!("worker.cancelled").to_string())
            }
            Err(e) => {
                let msg = self.em.fail(t!("worker.install_failed"), &e);
                (Outcome::Failed, t!("worker.install_failed_msg", error = msg))
            }
        };
        let _ = std::fs::remove_dir_all(paths::extract_dir());
        self.em.send(Event::OpFinished { outcome, summary });
        self.emit_overview();
    }

    fn run_jobs(&mut self, mut jobs: Vec<Job>, opts: &InstallOptions, extras: Extras) -> Result<Report> {
        let mut report = Report::default();
        self.wait_for_geostore()?;
        let mut hasp_pending = extras.hasp;
        let mut hasp: Option<PathBuf> = None;
        let mut i = 0;
        while i < jobs.len() {
            let job = jobs[i].clone();
            let (step, steps) = (i + 1, jobs.len());
            self.em.log(Level::Info, format!("{} – {} ({})", job.name, job.file.title, fmt::date(job.file.date)));
            let zip = self.fetch(&job, step, steps)?;
            match job.kind {
                JobKind::Editor { force } => {
                    let changed = self.install_editor(&job, &zip, force, step, steps)?;
                    if changed {
                        let version = local::editor_info().display_version.unwrap_or_default();
                        report.lines.push(t!("worker.editor_installed_line", version = version));
                        if extras.vcredist {
                            self.progress(step, steps, &job.name, t!("worker.stage_vcredist"), None, "", false);
                            let em = self.em.clone();
                            installer::run_vcredist(|l| em.log(Level::Info, l));
                        }
                        let rows = self.rows();
                        let mut follow: Vec<Job> = opts
                            .tracked
                            .iter()
                            .filter(|k| k.as_str() != EDITOR && !jobs.iter().any(|j| &j.key == *k))
                            .filter_map(|k| {
                                let row = rows.iter().find(|r| &r.key == k && r.detected)?;
                                Some(Job {
                                    key: k.clone(),
                                    name: row.info.name.clone(),
                                    file: row.latest.clone()?,
                                    kind: JobKind::Module { complete: false },
                                })
                            })
                            .collect();
                        if !follow.is_empty() {
                            sort_jobs(&mut follow);
                            self.em.log(
                                Level::Info,
                                t!("worker.reinstall_tracked"),
                            );
                            jobs.extend(follow);
                        }
                    } else {
                        report.lines.push(t!("worker.editor_skipped_line").into());
                    }
                }
                JobKind::Module { complete } => {
                    let dir = paths::extract_dir().join(&job.key);
                    let (line, warn) = self.install_module(&job, &zip, &dir, opts, complete, step, steps)?;
                    report.lines.push(line);
                    report.warnings += warn;
                    if hasp_pending {
                        if let Some(found) = installer::find_install_root(&dir).and_then(|r| installer::find_hasp(&r)) {
                            hasp_pending = false;
                            match stash_hasp(&found) {
                                Some(exe) => hasp = Some(exe),
                                None => self.run_hasp(&found, step, steps, &mut report),
                            }
                        }
                    }
                    installer::remove_dir(&dir);
                    if complete {
                        let newer_partial = self
                            .rows()
                            .into_iter()
                            .find(|r| r.key == job.key)
                            .and_then(|r| r.latest)
                            .filter(|p| p.id != job.file.id && p.date > job.file.date);
                        if let Some(p) = newer_partial {
                            self.em.log(Level::Info, t!("worker.newer_partial", name = job.name));
                            jobs.insert(
                                i + 1,
                                Job { key: job.key.clone(), name: job.name.clone(), file: p, kind: JobKind::Module { complete: false } },
                            );
                        }
                    }
                }
            }
            if !opts.keep_downloads {
                let _ = std::fs::remove_file(&zip);
            }
            self.emit_overview();
            i += 1;
        }
        if hasp_pending {
            report.lines.push(t!("worker.hasp_missing_line").into());
            report.warnings += 1;
        }
        if let Some(guid) = &extras.licence {
            let steps = jobs.len().max(1);
            self.progress(steps, steps, t!("worker.licence_package"), t!("worker.stage_licence"), None, "", false);
            match self.install_licence(guid) {
                Ok(line) => report.lines.push(line),
                Err(e) => {
                    let msg = friendly_error(&e);
                    self.em.log(Level::Warn, t!("worker.licence_failed", error = msg));
                    report.lines.push(t!("worker.licence_failed_line", error = msg));
                    report.warnings += 1;
                }
            }
        }
        if let Some(exe) = hasp {
            let steps = jobs.len().max(1);
            self.run_hasp(&exe, steps, steps, &mut report);
        }
        Ok(report)
    }

    fn run_hasp(&self, exe: &Path, step: usize, steps: usize, report: &mut Report) {
        self.progress(step, steps, t!("worker.hasp_package"), t!("worker.stage_hasp"), None, "", false);
        self.em.log(Level::Info, t!("worker.hasp_starting", path = exe.display()));
        match installer::run_interactive(exe) {
            Ok(code) => self.em.log(Level::Info, t!("worker.hasp_exit", code = code)),
            Err(e) => {
                report.warnings += 1;
                self.em.log(Level::Warn, friendly_error(&e));
            }
        }
    }

    fn send_licences(&self) {
        let account = crate::creds::load_licman().map(|(u, _)| u);
        let result = self.licman().and_then(|mut l| l.list_licences());
        if let Err(e) = &result {
            self.em.log(Level::Warn, t!("worker.licences_failed", error = friendly_error(e)));
        }
        self.em.send(Event::Licences { result: result.map_err(|e| friendly_error(&e)), account });
    }

    /// Signs in to the licence server with its own saved account if there is one, otherwise with the portal account.
    fn licman(&self) -> Result<Portal> {
        if let Some((user, pass)) = crate::creds::load_licman() {
            let mut licman = Portal::licman()?;
            licman.login(&user, &pass).context(t!("worker.licman_login_failed_account", user = user))?;
            return Ok(licman);
        }
        let (user, pass) = self
            .portal
            .as_ref()
            .and_then(Portal::credentials)
            .ok_or_else(|| anyhow!(t!("worker.not_logged_in")))?;
        let mut licman = Portal::licman()?;
        licman
            .login(&user, &pass)
            .context(t!("worker.licman_login_failed"))?;
        Ok(licman)
    }

    fn install_licence(&self, guid: &str) -> Result<String> {
        let dir = PathBuf::from(paths::GEOSTORE_DIR);
        if !dir.is_dir() {
            bail!(t!("worker.no_geostore_for_licence"));
        }
        let key = self.licman()?.download_licence(guid)?;
        let dest = dir.join("gsfl.lic");
        if std::fs::read_to_string(&dest).is_ok_and(|old| old.trim() == key) {
            self.em.log(Level::Info, t!("worker.licence_current", path = dest.display()));
            return Ok(t!("worker.licence_current_line").into());
        }
        std::fs::write(&dest, &key).with_context(|| t!("worker.write_failed", path = dest.display()))?;
        self.em.log(Level::Info, t!("worker.licence_saved", path = dest.display()));
        Ok(t!("worker.licence_saved_line").into())
    }

    fn wait_for_geostore(&self) -> Result<()> {
        if !local::is_geostore_running() {
            return Ok(());
        }
        self.em.log(Level::Warn, t!("worker.geostore_running"));
        self.em.send(Event::WaitingForGeoStore(true));
        while local::is_geostore_running() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        self.em.send(Event::WaitingForGeoStore(false));
        self.em.log(Level::Info, t!("worker.geostore_closed"));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn progress(&self, step: usize, steps: usize, package: &str, stage: impl Into<String>, fraction: Option<f32>, detail: impl Into<String>, cancellable: bool) {
        self.em.send(Event::Progress(Progress {
            kind: StageKind::Other,
            step,
            steps,
            package: package.to_string(),
            stage: stage.into(),
            fraction,
            detail: detail.into(),
            cancellable,
        }));
    }

    fn fetch(&mut self, job: &Job, step: usize, steps: usize) -> Result<PathBuf> {
        let name = format!("{}_{}", job.file.date.format("%Y%m%d"), sanitize(&job.file.title));
        let path = paths::downloads_dir().join(name);
        if path.is_file() && installer::zip_ok(&path) {
            self.em.log(Level::Info, t!("worker.cached_download", path = path.display()));
            return Ok(path);
        }
        self.progress(step, steps, &job.name, t!("worker.stage_preparing_download", file = job.file.title), None, "", true);
        let expected = self.state.sizes.get(&job.file.title).copied();
        let cancel = self.cancel.clone();
        let file = job.file.clone();
        let package = job.name.clone();
        let started = Instant::now();
        let total = self.with_portal(|portal, em| {
            let url = portal.resolve_download(&file)?;
            let mut last = Instant::now() - Duration::from_secs(1);
            let mut samples: VecDeque<(Instant, u64)> = VecDeque::new();
            portal.download(&url, &path, &cancel, |done| {
                let now = Instant::now();
                if now.duration_since(last) < Duration::from_millis(150) {
                    return;
                }
                last = now;
                samples.push_back((now, done));
                while samples.len() > 2 && now.duration_since(samples[0].0) > Duration::from_secs(4) {
                    samples.pop_front();
                }
                let speed = match (samples.front(), samples.back()) {
                    (Some(a), Some(b)) => {
                        let dt = b.0.duration_since(a.0).as_secs_f64();
                        if dt > 0.3 { (b.1 - a.1) as f64 / dt } else { 0.0 }
                    }
                    _ => 0.0,
                };
                let mut detail = match expected {
                    Some(e) => t!("worker.downloaded_of_about", done = fmt::mb(done), total = fmt::mb(e)),
                    None => fmt::mb(done),
                };
                if speed > 0.0 {
                    detail.push_str(&format!(" · {}/s", fmt::mb(speed as u64)));
                    if let Some(e) = expected.filter(|e| *e > done) {
                        detail.push_str(&format!(" · {}", t!("worker.time_left", time = fmt::duration(((e - done) as f64 / speed) as u64))));
                    }
                }
                em.send(Event::Progress(Progress {
                    kind: StageKind::Download,
                    step,
                    steps,
                    package: package.clone(),
                    stage: t!("worker.stage_download", file = file.title),
                    fraction: expected.map(|e| (done as f64 / e as f64).min(0.99) as f32),
                    detail,
                    cancellable: true,
                }));
            })
        })?;
        let title = job.file.title.clone();
        self.state = State::update(|s| {
            s.sizes.insert(title, total);
        });
        self.em.log(
            Level::Info,
            t!("worker.downloaded", size = fmt::mb(total), time = fmt::duration(started.elapsed().as_secs())),
        );
        if !installer::zip_ok(&path) {
            let _ = std::fs::remove_file(&path);
            return Err(anyhow!(t!("worker.download_corrupt", file = job.file.title)));
        }
        Ok(path)
    }

    fn extract(&self, job: &Job, zip: &Path, dir: &Path, step: usize, steps: usize) -> Result<()> {
        let em = self.em.clone();
        let mut last = Instant::now() - Duration::from_secs(1);
        installer::extract(zip, dir, &self.cancel, |done, total| {
            if last.elapsed() < Duration::from_millis(120) && done != total {
                return;
            }
            last = Instant::now();
            em.send(Event::Progress(Progress {
                kind: StageKind::Extract,
                step,
                steps,
                package: job.name.clone(),
                stage: t!("worker.stage_extract").into(),
                fraction: Some(done as f32 / total.max(1) as f32),
                detail: t!("worker.files_progress", done = done, total = total),
                cancellable: true,
            }));
        })
    }

    fn install_editor(&mut self, job: &Job, zip: &Path, force: bool, step: usize, steps: usize) -> Result<bool> {
        let dir = paths::extract_dir().join(EDITOR);
        self.extract(job, zip, &dir, step, steps)?;
        let msi = installer::find_file(&dir, |n| n.ends_with(".msi"))
            .context(t!("worker.no_msi"))?;
        let new_version = local::msi_product_version(&msi);
        let before = local::editor_info();
        self.em.log(
            Level::Info,
            t!(
                "worker.versions",
                new = new_version.as_deref().unwrap_or(t!("worker.version_unknown")),
                current = before.display_version.as_deref().unwrap_or(t!("worker.version_none")),
            ),
        );
        if !force && before.installed {
            if let (Some(new), Some(cur)) = (&new_version, &before.display_version) {
                if local::compare_versions(new, cur).is_le() {
                    self.record(EDITOR, &job.file, before.display_version.clone(), false);
                    installer::remove_dir(&dir);
                    return Ok(false);
                }
            }
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        for code in &before.product_codes {
            self.progress(step, steps, &job.name, t!("worker.stage_uninstall"), None, code.as_str(), false);
            self.em.log(Level::Info, t!("worker.uninstalling", code = code));
            installer::msi_uninstall(code)?;
        }
        let version_label = new_version.clone().unwrap_or_default();
        self.progress(step, steps, &job.name, t!("worker.stage_install_editor", version = version_label), None, t!("worker.install_background"), false);
        let reboot = installer::msi_install(&msi)?;
        if reboot {
            self.em.log(Level::Warn, t!("worker.reboot_recommended"));
        }
        let after = local::editor_info();
        if !after.installed {
            return Err(anyhow!(t!("worker.editor_missing_after")));
        }
        self.em.log(
            Level::Success,
            t!("worker.editor_installed", version = after.display_version.as_deref().unwrap_or("")),
        );
        self.record(EDITOR, &job.file, after.display_version, false);
        installer::remove_dir(&dir);
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn install_module(&mut self, job: &Job, zip: &Path, dir: &Path, opts: &InstallOptions, complete: bool, step: usize, steps: usize) -> Result<(String, usize)> {
        self.extract(job, zip, dir, step, steps)?;
        let root = installer::find_install_root(dir)
            .context(t!("worker.no_davka"))?;

        let previous = config::load_manifest(&job.key);
        let backup_dir = PathBuf::from(paths::BACKUP_ROOT)
            .join(format!("{}_{}", Local::now().format("%Y-%m-%d_%H-%M-%S"), job.key));
        let em = self.em.clone();
        let name = job.name.clone();
        let mut last = Instant::now() - Duration::from_secs(1);
        let (backed_up, manifest) = installer::scan_konfig(
            &root.join("V6-KONFIG"),
            previous.as_ref(),
            opts.backup.then_some(backup_dir.as_path()),
            &self.cancel,
            |done, total| {
                if last.elapsed() < Duration::from_millis(120) {
                    return;
                }
                last = Instant::now();
                em.send(Event::Progress(Progress {
                    kind: StageKind::Other,
                    step,
                    steps,
                    package: name.clone(),
                    stage: t!("worker.stage_konfig").into(),
                    fraction: Some(done as f32 / total.max(1) as f32),
                    detail: t!("worker.files_progress", done = done, total = total),
                    cancellable: true,
                }));
            },
        )?;
        if backed_up > 0 {
            self.em.log(
                Level::Info,
                t!("worker.backed_up", count = backed_up, path = backup_dir.display()),
            );
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }

        let bat = installer::prepare_davka(&root)?;
        self.progress(step, steps, &job.name, t!("worker.stage_copy"), None, t!("worker.davka_running"), false);
        let em = self.em.clone();
        let mut count = 0usize;
        let mut last = Instant::now();
        let package = job.name.clone();
        let outcome = installer::run_davka(&root, &bat, |line| {
            count += 1;
            em.log(Level::Detail, line);
            if last.elapsed() > Duration::from_millis(200) {
                last = Instant::now();
                em.send(Event::Progress(Progress {
                    kind: StageKind::Other,
                    step,
                    steps,
                    package: package.clone(),
                    stage: t!("worker.stage_copy").into(),
                    fraction: None,
                    detail: t!("worker.davka_lines", count = count),
                    cancellable: false,
                }));
            }
        })?;

        config::save_manifest(&job.key, &manifest);
        let info = crate::catalog::module_info(&job.key);
        let version = local::detect_module(&info, &DavkaLog::read()).version;
        self.record(&job.key, &job.file, version.clone(), complete);

        let mut warnings = 0;
        let mut line = match version {
            Some(v) => t!("worker.module_installed_version", name = job.name, file = job.file.title, version = v),
            None => t!("worker.module_installed", name = job.name, file = job.file.title),
        };
        if backed_up > 0 {
            line.push_str(&t!("worker.module_backed_up", count = backed_up));
        }
        if outcome.failed > 0 || outcome.errors > 0 {
            warnings += 1;
            let msg = t!("worker.copy_failed", name = job.name, failed = outcome.failed, errors = outcome.errors);
            self.em.log(Level::Warn, msg.clone());
            line = format!("• {msg}");
        } else {
            self.em.log(Level::Success, t!("worker.module_done", name = job.name, count = outcome.copied));
        }
        Ok((line, warnings))
    }

    fn record(&mut self, key: &str, file: &RemoteFile, version: Option<String>, complete: bool) {
        let record = InstalledRecord {
            portal_date: file.date,
            record_id: file.id.clone(),
            file: file.title.clone(),
            installed_at: Local::now(),
            version,
            complete,
        };
        self.state = State::update(|s| {
            s.packages.insert(key.to_string(), record);
        });
    }
}

fn sort_jobs(jobs: &mut [Job]) {
    jobs.sort_by(|a, b| {
        let ea = matches!(a.kind, JobKind::Editor { .. });
        let eb = matches!(b.kind, JobKind::Editor { .. });
        eb.cmp(&ea).then(a.file.date.cmp(&b.file.date)).then(a.key.cmp(&b.key))
    });
}

fn stash_hasp(exe: &Path) -> Option<PathBuf> {
    let dest = paths::extract_dir().join("_hasp");
    installer::remove_dir(&dest);
    std::fs::rename(exe.parent()?, &dest).ok()?;
    Some(dest.join(exe.file_name()?))
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect()
}
