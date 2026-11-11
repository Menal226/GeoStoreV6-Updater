use crate::catalog::EDITOR;
use crate::config::{self, InstalledRecord, State};
use crate::fmt;
use crate::installer;
use crate::local::{self, DavkaLog};
use crate::logfile::{self, Level, LogLine};
use crate::logic::{self, PackageRow};
use crate::paths;
use crate::portal::{self, Cancelled, Portal, RemoteFile};
use anyhow::{anyhow, Context, Result};
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
}

impl CleanPlan {
    pub fn is_empty(&self) -> bool {
        !self.editor && self.modules.is_empty()
    }
}

pub enum Cmd {
    Login { user: String, pass: String, remember: bool },
    Logout,
    Check,
    Install { keys: Vec<String>, opts: InstallOptions },
    CleanInstall { plan: CleanPlan, opts: InstallOptions },
    MarkCurrent(String),
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
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
    OpStarted { title: String },
    Progress(Progress),
    WaitingForGeoStore(bool),
    Log(LogLine),
    OpFinished { ok: bool, summary: String },
}

#[derive(Clone)]
pub struct Emitter {
    tx: Sender<Event>,
    ctx: eframe::egui::Context,
}

impl Emitter {
    pub fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ctx.request_repaint();
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

pub fn spawn(ctx: eframe::egui::Context) -> (Sender<Cmd>, Receiver<Event>, Arc<AtomicBool>) {
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
}

struct Worker {
    em: Emitter,
    cancel: Arc<AtomicBool>,
    portal: Option<Portal>,
    files: Vec<RemoteFile>,
    state: State,
}

pub fn friendly_error(e: &anyhow::Error) -> String {
    for cause in e.chain() {
        if let Some(re) = cause.downcast_ref::<reqwest::Error>() {
            if re.is_connect() || re.is_timeout() {
                return "Nelze se spojit s portálem portal.geostore.cz. Zkontrolujte připojení k internetu.".into();
            }
        }
    }
    let mut parts: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    parts.dedup();
    parts.join(" – ")
}

impl Worker {
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Login { user, pass, remember } => self.login(user, pass, remember),
            Cmd::Logout => {
                self.portal = None;
                self.files.clear();
                crate::creds::delete();
                self.em.log(Level::Info, "Odhlášeno, uložené přihlašovací údaje byly odstraněny.");
                self.em.send(Event::LoggedOut);
            }
            Cmd::Check => self.check(),
            Cmd::Install { keys, opts } => {
                let jobs = self.update_jobs(&keys, &opts);
                self.run_operation("Instalace aktualizací", jobs, opts, Extras { hasp: false, vcredist: false });
            }
            Cmd::CleanInstall { plan, opts } => {
                let jobs = self.clean_jobs(&plan);
                let extras = Extras { hasp: plan.hasp, vcredist: plan.vcredist };
                self.run_operation("Nová instalace", jobs, opts, extras);
            }
            Cmd::MarkCurrent(key) => {
                if let Some(row) = self.rows().into_iter().find(|r| r.key == key) {
                    if let Some(f) = &row.latest {
                        self.record(&key, f, row.local_version.clone(), false);
                        self.em.log(
                            Level::Info,
                            format!("{} označen jako aktuální ({}).", row.info.name, fmt::date(f.date)),
                        );
                    }
                }
                self.emit_overview();
            }
        }
    }

    fn login(&mut self, user: String, pass: String, remember: bool) {
        let result = Portal::new().and_then(|mut p| p.login(&user, &pass).map(|_| p));
        match result {
            Ok(p) => {
                if remember {
                    if let Err(e) = crate::creds::save(&user, &pass) {
                        self.em.log(Level::Warn, format!("Přihlašovací údaje se nepodařilo uložit: {e}"));
                    }
                } else {
                    crate::creds::delete();
                }
                let name = p.user_name.clone();
                let roles: Vec<String> = p.roles.iter().map(|r| r.name.clone()).collect();
                self.portal = Some(p);
                self.em.log(
                    Level::Detail,
                    format!("Přihlášen uživatel {name} (licence: {})", roles.join(", ")),
                );
                self.em.send(Event::LoggedIn { user: name });
                self.check();
            }
            Err(e) => {
                let msg = self.em.fail("Přihlášení se nezdařilo", &e);
                self.em.send(Event::LoginFailed(msg));
            }
        }
    }

    fn check(&mut self) {
        self.em.send(Event::Checking);
        match self.with_portal(|p, _| p.list_files()) {
            Ok(files) => {
                self.files = files;
                self.state.last_check = Some(Local::now());
                self.state.save();
                let rows = self.rows();
                let pending = rows
                    .iter()
                    .filter(|r| r.status == logic::Status::UpdateAvailable)
                    .count();
                let outcome = match pending {
                    0 => "vše aktuální".to_string(),
                    1..=4 => format!("k dispozici {pending} aktualizace"),
                    _ => format!("k dispozici {pending} aktualizací"),
                };
                self.em.log(
                    Level::Info,
                    format!("Kontrola dokončena: {outcome} ({} {} na portálu)", rows.len(), fmt::packages(rows.len())),
                );
                self.emit_overview();
            }
            Err(e) => {
                let msg = self.em.fail("Kontrola aktualizací selhala", &e);
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
            .ok_or_else(|| anyhow!("Nejste přihlášeni k portálu."))?;
        let result = f(&mut portal, &self.em);
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

    fn run_operation(&mut self, title: &str, jobs: Vec<Job>, opts: InstallOptions, extras: Extras) {
        self.cancel.store(false, Ordering::Relaxed);
        self.em.send(Event::OpStarted { title: title.to_string() });
        self.em.log(Level::Info, format!("{title}: zahájeno."));
        let result = if jobs.is_empty() {
            Err(anyhow!("Nebyl vybrán žádný balíček k instalaci."))
        } else {
            self.run_jobs(jobs, &opts, extras)
        };
        self.em.send(Event::WaitingForGeoStore(false));
        let (ok, summary) = match result {
            Ok(report) => {
                let ok = report.warnings == 0;
                let head = if ok {
                    "Instalace byla úspěšně dokončena."
                } else {
                    "Instalace byla dokončena s upozorněním."
                };
                let mut text = head.to_string();
                for l in &report.lines {
                    text.push('\n');
                    text.push_str(l);
                }
                self.em.log(if ok { Level::Success } else { Level::Warn }, text.clone());
                (ok, text)
            }
            Err(e) if portal::is_cancelled(&e) => {
                self.em.log(Level::Warn, "Operace byla zrušena.");
                (false, "Operace byla zrušena.".to_string())
            }
            Err(e) => {
                let msg = self.em.fail("Instalace selhala", &e);
                (false, format!("Instalace selhala: {msg}"))
            }
        };
        let _ = std::fs::remove_dir_all(paths::extract_dir());
        self.em.send(Event::OpFinished { ok, summary });
        self.emit_overview();
    }

    fn run_jobs(&mut self, mut jobs: Vec<Job>, opts: &InstallOptions, extras: Extras) -> Result<Report> {
        let mut report = Report::default();
        self.wait_for_geostore()?;
        let mut hasp_pending = extras.hasp;
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
                        report.lines.push(format!("• GeoStore V6: nainstalována verze {version}"));
                        if extras.vcredist {
                            self.progress(step, steps, &job.name, "Instalace Microsoft Visual C++ 2010/2012", None, "", false);
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
                                "Po aktualizaci editoru budou znovu nainstalována sledovaná řešení, aby jejich knihovny zůstaly v pořádku.",
                            );
                            jobs.extend(follow);
                        }
                    } else {
                        report.lines.push("• GeoStore V6: nainstalovaná verze je aktuální, instalace přeskočena".into());
                    }
                }
                JobKind::Module { complete } => {
                    let dir = paths::extract_dir().join(&job.key);
                    let (line, warn) = self.install_module(&job, &zip, &dir, opts, complete, step, steps)?;
                    report.lines.push(line);
                    report.warnings += warn;
                    if hasp_pending {
                        if let Some(root) = installer::find_install_root(&dir) {
                            if let Some(hasp) = installer::find_hasp(&root) {
                                hasp_pending = false;
                                self.progress(step, steps, &job.name, "Instalace ovladače HW klíče – dokončete prosím průvodce", None, "", false);
                                self.em.log(Level::Info, format!("Spouštím instalaci ovladače HW klíče: {}", hasp.display()));
                                match installer::run_interactive(&hasp) {
                                    Ok(code) => self.em.log(Level::Info, format!("Instalátor ovladače HW klíče skončil s kódem {code}.")),
                                    Err(e) => {
                                        report.warnings += 1;
                                        self.em.log(Level::Warn, friendly_error(&e));
                                    }
                                }
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
                            self.em.log(Level::Info, format!("Pro {} existuje novější částečná aktualizace, bude doinstalována.", job.name));
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
            report.lines.push("• Ovladač HW klíče: instalátor nebyl v balíčcích nalezen".into());
            report.warnings += 1;
        }
        Ok(report)
    }

    fn wait_for_geostore(&self) -> Result<()> {
        if !local::is_geostore_running() {
            return Ok(());
        }
        self.em.log(Level::Warn, "GeoStore V6 je spuštěn – instalace počká na jeho ukončení.");
        self.em.send(Event::WaitingForGeoStore(true));
        while local::is_geostore_running() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(Cancelled.into());
            }
            std::thread::sleep(Duration::from_millis(1500));
        }
        self.em.send(Event::WaitingForGeoStore(false));
        self.em.log(Level::Info, "GeoStore V6 byl ukončen, pokračuji v instalaci.");
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn progress(&self, step: usize, steps: usize, package: &str, stage: impl Into<String>, fraction: Option<f32>, detail: impl Into<String>, cancellable: bool) {
        self.em.send(Event::Progress(Progress {
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
            self.em.log(Level::Info, format!("Použit dříve stažený soubor {}.", path.display()));
            return Ok(path);
        }
        self.progress(step, steps, &job.name, format!("Připravuji stažení {}", job.file.title), None, "", true);
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
                let mut detail = fmt::mb(done);
                if let Some(e) = expected {
                    detail.push_str(&format!(" z přibližně {}", fmt::mb(e)));
                }
                if speed > 0.0 {
                    detail.push_str(&format!(" · {}/s", fmt::mb(speed as u64)));
                    if let Some(e) = expected.filter(|e| *e > done) {
                        detail.push_str(&format!(" · zbývá asi {}", fmt::duration(((e - done) as f64 / speed) as u64)));
                    }
                }
                em.send(Event::Progress(Progress {
                    step,
                    steps,
                    package: package.clone(),
                    stage: format!("Stahování {}", file.title),
                    fraction: expected.map(|e| (done as f64 / e as f64).min(0.99) as f32),
                    detail,
                    cancellable: true,
                }));
            })
        })?;
        self.state.sizes.insert(job.file.title.clone(), total);
        self.state.save();
        self.em.log(
            Level::Info,
            format!("Staženo {} za {}.", fmt::mb(total), fmt::duration(started.elapsed().as_secs())),
        );
        if !installer::zip_ok(&path) {
            let _ = std::fs::remove_file(&path);
            return Err(anyhow!("Stažený soubor {} je poškozený.", job.file.title));
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
                step,
                steps,
                package: job.name.clone(),
                stage: "Rozbalování balíčku".into(),
                fraction: Some(done as f32 / total.max(1) as f32),
                detail: format!("{done} z {total} souborů"),
                cancellable: true,
            }));
        })
    }

    fn install_editor(&mut self, job: &Job, zip: &Path, force: bool, step: usize, steps: usize) -> Result<bool> {
        let dir = paths::extract_dir().join(EDITOR);
        self.extract(job, zip, &dir, step, steps)?;
        let msi = installer::find_file(&dir, |n| n.ends_with(".msi"))
            .context("V balíčku editoru nebyl nalezen instalační soubor MSI.")?;
        let new_version = local::msi_product_version(&msi);
        let before = local::editor_info();
        self.em.log(
            Level::Info,
            format!(
                "Verze v balíčku: {}, nainstalovaná verze: {}",
                new_version.as_deref().unwrap_or("neznámá"),
                before.display_version.as_deref().unwrap_or("žádná")
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
            self.progress(step, steps, &job.name, "Odinstalace předchozí verze GeoStore V6", None, code.as_str(), false);
            self.em.log(Level::Info, format!("Odinstalace předchozí verze ({code})."));
            installer::msi_uninstall(code)?;
        }
        let version_label = new_version.clone().unwrap_or_default();
        self.progress(step, steps, &job.name, format!("Instalace GeoStore V6 {version_label}"), None, "Instalace probíhá na pozadí, může trvat několik minut.", false);
        let reboot = installer::msi_install(&msi)?;
        if reboot {
            self.em.log(Level::Warn, "Instalátor GeoStore V6 doporučuje restartovat počítač.");
        }
        let after = local::editor_info();
        if !after.installed {
            return Err(anyhow!("Po instalaci nebyl GeoStore V6 nalezen."));
        }
        self.em.log(
            Level::Success,
            format!("GeoStore V6 {} nainstalován.", after.display_version.as_deref().unwrap_or("")),
        );
        self.record(EDITOR, &job.file, after.display_version, false);
        installer::remove_dir(&dir);
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    fn install_module(&mut self, job: &Job, zip: &Path, dir: &Path, opts: &InstallOptions, complete: bool, step: usize, steps: usize) -> Result<(String, usize)> {
        self.extract(job, zip, dir, step, steps)?;
        let root = installer::find_install_root(dir)
            .context("V balíčku nebyla nalezena instalační dávka davka.bat.")?;

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
                    step,
                    steps,
                    package: name.clone(),
                    stage: "Kontrola upravených konfiguračních souborů v C:\\V6-KONFIG".into(),
                    fraction: Some(done as f32 / total.max(1) as f32),
                    detail: format!("{done} z {total} souborů"),
                    cancellable: true,
                }));
            },
        )?;
        if backed_up > 0 {
            self.em.log(
                Level::Info,
                format!("Zálohováno {backed_up} upravených souborů z C:\\V6-KONFIG do {}.", backup_dir.display()),
            );
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }

        let bat = installer::prepare_davka(&root)?;
        self.progress(step, steps, &job.name, "Kopírování souborů do počítače", None, "Probíhá instalační dávka balíčku (davka.bat).", false);
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
                    step,
                    steps,
                    package: package.clone(),
                    stage: "Kopírování souborů do počítače".into(),
                    fraction: None,
                    detail: format!("Zpracováno {count} řádků výstupu instalační dávky"),
                    cancellable: false,
                }));
            }
        })?;

        config::save_manifest(&job.key, &manifest);
        let info = crate::catalog::module_info(&job.key);
        let version = local::detect_module(&info, &DavkaLog::read()).version;
        self.record(&job.key, &job.file, version.clone(), complete);

        let mut warnings = 0;
        let mut line = format!(
            "• {}: nainstalováno z {}{}",
            job.name,
            job.file.title,
            version.map(|v| format!(" (verze {v})")).unwrap_or_default()
        );
        if backed_up > 0 {
            line.push_str(&format!(", zálohováno {backed_up} upravených souborů"));
        }
        if outcome.failed > 0 || outcome.errors > 0 {
            warnings += 1;
            let msg = format!(
                "{}: při kopírování selhalo {} souborů ({} chyb). Podrobnosti najdete v davka.log.",
                job.name, outcome.failed, outcome.errors
            );
            self.em.log(Level::Warn, msg.clone());
            line = format!("• {msg}");
        } else {
            self.em.log(Level::Success, format!("{}: instalace dokončena, zkopírováno {} souborů.", job.name, outcome.copied));
        }
        Ok((line, warnings))
    }

    fn record(&mut self, key: &str, file: &RemoteFile, version: Option<String>, complete: bool) {
        self.state.packages.insert(
            key.to_string(),
            InstalledRecord {
                portal_date: file.date,
                record_id: file.id.clone(),
                file: file.title.clone(),
                installed_at: Local::now(),
                version,
                complete,
            },
        );
        self.state.save();
    }
}

fn sort_jobs(jobs: &mut [Job]) {
    jobs.sort_by(|a, b| {
        let ea = matches!(a.kind, JobKind::Editor { .. });
        let eb = matches!(b.kind, JobKind::Editor { .. });
        eb.cmp(&ea).then(a.file.date.cmp(&b.file.date)).then(a.key.cmp(&b.key))
    });
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect()
}
