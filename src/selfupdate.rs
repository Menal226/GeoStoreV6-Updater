use crate::logfile::{self, Level};
use crate::{local, paths};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

const LATEST_RELEASE: &str = "https://api.github.com/repos/Menal226/GeoStoreV6-Updater/releases/latest";
const ASSET: &str = "GeoStoreUpdater.exe";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const RECHECK: Duration = Duration::from_secs(24 * 3600);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// The program's own path, also when this process runs from the copy an update renamed aside.
pub fn app_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = exe.file_name()?.to_string_lossy().to_string();
    Some(match name.strip_suffix(".old.exe") {
        Some(stem) => exe.with_file_name(format!("{stem}.exe")),
        None => exe,
    })
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

fn staged_path() -> PathBuf {
    paths::sub("update").join(ASSET)
}

/// Checks GitHub daily without blocking the caller, until a new version is in place.
pub fn spawn(check_now: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        if !check_now {
            std::thread::sleep(RECHECK);
        }
        while !run() {
            std::thread::sleep(RECHECK);
        }
    });
}

/// True once a newer version has replaced the program on disk.
pub fn run() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    match update() {
        Ok(replaced) => replaced,
        Err(e) => {
            logfile::write(Level::Detail, &format!("Aktualizace aplikace se nezdařila: {e:#}"));
            false
        }
    }
}

fn update() -> Result<bool> {
    let exe = app_exe().context("Nelze zjistit cestu k programu")?;
    let _ = std::fs::remove_file(old_path(&exe));

    let staged = staged_path();
    if staged.is_file() {
        let newer = local::file_version(&staged).is_some_and(|v| local::compare_versions(&v, CURRENT) == Ordering::Greater);
        if newer {
            return replace(&exe, &staged).map(|_| true);
        }
        let _ = std::fs::remove_file(&staged);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("GeoStoreUpdater/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()?;
    let resp = client
        .get(LATEST_RELEASE)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    let release: Release = resp.error_for_status()?.json()?;
    let version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    if local::compare_versions(&version, CURRENT) != Ordering::Greater {
        return Ok(false);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET))
        .with_context(|| format!("Vydání {} neobsahuje soubor {ASSET}", release.tag_name))?;

    let part = staged.with_extension("part");
    let mut resp = client.get(&asset.browser_download_url).send()?.error_for_status()?;
    let written = {
        let mut file = File::create(&part)?;
        resp.copy_to(&mut file)?
    };
    if written != asset.size {
        let _ = std::fs::remove_file(&part);
        bail!("Stažený soubor má {written} B místo {} B", asset.size);
    }
    std::fs::rename(&part, &staged)?;
    let file_version = local::file_version(&staged).unwrap_or_default();
    if local::compare_versions(&file_version, &version) != Ordering::Equal {
        let _ = std::fs::remove_file(&staged);
        bail!("Stažený soubor má verzi {file_version}, očekávána {version}");
    }
    replace(&exe, &staged).map(|_| true)
}

/// A running exe cannot be overwritten but can be renamed, so the old one is moved aside and the new one takes its name.
fn replace(exe: &Path, staged: &Path) -> Result<()> {
    let version = local::file_version(staged).unwrap_or_default();
    if std::fs::copy(staged, exe).is_err() {
        let old = old_path(exe);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).context("Nelze přejmenovat spuštěný program")?;
        if let Err(e) = std::fs::copy(staged, exe) {
            let _ = std::fs::rename(&old, exe);
            return Err(e).context("Nelze zapsat novou verzi programu");
        }
    }
    let _ = std::fs::remove_file(staged);
    logfile::write(Level::Info, &format!("Připravena nová verze aplikace {version}, použije se při příštím spuštění."));
    Ok(())
}
