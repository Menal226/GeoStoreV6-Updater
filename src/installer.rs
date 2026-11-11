use crate::config::Manifest;
use crate::local;
use crate::paths;
use crate::portal::Cancelled;
use anyhow::{bail, Context, Result};
use chrono::{Local, NaiveDate, TimeZone};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn zip_ok(p: &Path) -> bool {
    File::open(p)
        .ok()
        .and_then(|f| zip::ZipArchive::new(BufReader::new(f)).ok())
        .is_some_and(|z| !z.is_empty())
}

pub fn remove_dir(p: &Path) {
    if p.exists() {
        let _ = fs::remove_dir_all(p);
    }
}

pub fn extract(
    zip_path: &Path,
    dest: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<()> {
    remove_dir(dest);
    fs::create_dir_all(dest)?;
    let file = File::open(zip_path)
        .with_context(|| format!("Nelze otevřít {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .context("Stažený soubor není platný ZIP archiv")?;
    let total = archive.len();
    for i in 0..total {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        {
            let mut f = BufWriter::new(File::create(&out)?);
            std::io::copy(&mut entry, &mut f)
                .with_context(|| format!("Chyba při rozbalování {}", out.display()))?;
        }
        if let Some(dt) = entry.last_modified() {
            if let Some(ts) = zip_time(&dt) {
                let _ = filetime::set_file_mtime(&out, filetime::FileTime::from_unix_time(ts, 0));
            }
        }
        if i % 20 == 0 {
            progress(i, total);
        }
    }
    progress(total, total);
    Ok(())
}

fn zip_time(dt: &zip::DateTime) -> Option<i64> {
    let date = NaiveDate::from_ymd_opt(dt.year() as i32, dt.month() as u32, dt.day() as u32)?;
    let ndt = date.and_hms_opt(dt.hour() as u32, dt.minute() as u32, dt.second() as u32)?;
    Local.from_local_datetime(&ndt).earliest().map(|t| t.timestamp())
}

pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => out.push(e.path()),
                _ => {}
            }
        }
    }
    out
}

pub fn find_install_root(dir: &Path) -> Option<PathBuf> {
    let mut level = vec![dir.to_path_buf()];
    for _ in 0..4 {
        let mut next = Vec::new();
        for d in &level {
            if d.join("davka.bat").is_file() {
                return Some(d.clone());
            }
            if let Ok(rd) = fs::read_dir(d) {
                next.extend(rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()));
            }
        }
        level = next;
    }
    None
}

pub fn find_file(dir: &Path, pred: impl Fn(&str) -> bool) -> Option<PathBuf> {
    walk_files(dir).into_iter().find(|p| {
        p.file_name()
            .map(|n| pred(&n.to_string_lossy().to_lowercase()))
            .unwrap_or(false)
    })
}

fn hash_file(p: &Path) -> std::io::Result<(u64, u64)> {
    let mut f = File::open(p)?;
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    Ok((size, hasher.digest()))
}

pub fn scan_konfig(
    package_konfig: &Path,
    previous: Option<&Manifest>,
    backup_dir: Option<&Path>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<(usize, Manifest)> {
    let mut manifest = Manifest::new();
    let mut backed_up = 0;
    if !package_konfig.is_dir() {
        return Ok((0, manifest));
    }
    let files = walk_files(package_konfig);
    let total = files.len();
    for (i, file) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let Ok(rel) = file.strip_prefix(package_konfig) else {
            continue;
        };
        let key = rel.to_string_lossy().to_lowercase();
        let packaged = hash_file(file)?;
        manifest.insert(key.clone(), packaged);
        if let Some(backup_dir) = backup_dir {
            let local = Path::new(paths::KONFIG_DIR).join(rel);
            if local.is_file() {
                if let Ok(current) = hash_file(&local) {
                    let differs = current != packaged;
                    let untouched = previous
                        .and_then(|m| m.get(&key))
                        .is_some_and(|installed| *installed == current);
                    if differs && !untouched {
                        let target = backup_dir.join(rel);
                        if let Some(parent) = target.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::copy(&local, &target).with_context(|| {
                            format!("Nelze zálohovat {}", local.display())
                        })?;
                        backed_up += 1;
                    }
                }
            }
        }
        if i % 25 == 0 {
            progress(i, total);
        }
    }
    Ok((backed_up, manifest))
}

fn find_ci(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&i| haystack[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

/// Writes an unattended copy of the package's davka.bat (Windows-1250 bytes are kept as they are).
pub fn prepare_davka(root: &Path) -> Result<PathBuf> {
    let original = fs::read(root.join("davka.bat")).context("V balíčku chybí davka.bat")?;

    let desktop = local::desktop_dir();
    let default_desktop = PathBuf::from(local::expand_env(r"%USERPROFILE%\Desktop"));
    let desktop_bytes = if desktop.to_string_lossy().to_lowercase()
        != default_desktop.to_string_lossy().to_lowercase()
    {
        let text = desktop.to_string_lossy().into_owned();
        let (bytes, _, had_errors) = encoding_rs::WINDOWS_1250.encode(&text);
        (!had_errors).then(|| bytes.into_owned())
    } else {
        None
    };

    let mut out: Vec<u8> = b"@cd /d \"%~dp0\"\r\n".to_vec();
    for raw in original.split(|b| *b == b'\n') {
        let line = raw.strip_suffix(b"\r").unwrap_or(raw);
        let trimmed = line.trim_ascii();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with(b".\\gsv6\\setup.exe") || lower == b"pause" {
            continue;
        }
        if lower.starts_with(b"set /p ") {
            out.extend_from_slice(b"set smaz=N\r\n");
            continue;
        }
        let mut line = line.to_vec();
        if let Some(desk) = &desktop_bytes {
            let needle = b"%userprofile%\\Desktop";
            if let Some(pos) = find_ci(&line, needle) {
                line.splice(pos..pos + needle.len(), desk.iter().copied());
            }
        }
        out.extend_from_slice(&line);
        out.extend_from_slice(b"\r\n");
    }
    let target = root.join("davka_updater.bat");
    fs::write(&target, out)?;
    Ok(target)
}

#[derive(Debug, Default, Clone)]
pub struct DavkaOutcome {
    pub copied: u64,
    pub failed: u64,
    pub errors: u32,
}

fn decode_line(b: &[u8]) -> String {
    let (s, _, _) = encoding_rs::WINDOWS_1250.decode(b);
    s.trim_end_matches(['\r', '\n']).to_string()
}

fn parse_robocopy(line: &str, out: &mut DavkaOutcome) {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("Files :") {
        let nums: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if nums.len() >= 5 {
            out.copied += nums[1];
            out.failed += nums[4];
        }
    } else if t.contains("ERROR ") && t.contains("(0x") {
        out.errors += 1;
    }
}

pub fn run_davka(root: &Path, bat: &Path, mut on_line: impl FnMut(String)) -> Result<DavkaOutcome> {
    let name = bat.file_name().context("Neplatná cesta k dávce")?;
    let mut child = Command::new("cmd.exe")
        .arg("/d")
        .arg("/c")
        .arg(name)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("Nelze spustit instalační dávku")?;

    let stderr = child.stderr.take().context("stderr")?;
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let mut r = BufReader::new(stderr);
        let mut buf = Vec::new();
        while r.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
            let _ = tx.send(decode_line(&buf));
            buf.clear();
        }
    });

    let mut outcome = DavkaOutcome::default();
    let mut r = BufReader::new(child.stdout.take().context("stdout")?);
    let mut buf = Vec::new();
    while r.read_until(b'\n', &mut buf)? > 0 {
        let line = decode_line(&buf);
        buf.clear();
        parse_robocopy(&line, &mut outcome);
        if !line.trim().is_empty() {
            on_line(line);
        }
        while let Ok(err) = rx.try_recv() {
            if !err.trim().is_empty() {
                on_line(err);
            }
        }
    }
    let _ = reader.join();
    for err in rx.try_iter() {
        if !err.trim().is_empty() {
            on_line(err);
        }
    }
    child.wait()?;
    Ok(outcome)
}

fn msi_log(name: &str) -> PathBuf {
    paths::logs_dir().join(format!("{}_{}.log", name, Local::now().format("%Y%m%d_%H%M%S")))
}

pub fn msi_uninstall(product_code: &str) -> Result<()> {
    let log = msi_log("msi_uninstall");
    let status = Command::new("msiexec.exe")
        .args(["/x", product_code, "/qn", "/norestart", "/l*v"])
        .arg(&log)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("Nelze spustit msiexec")?;
    match status.code() {
        Some(0) | Some(1605) | Some(3010) | Some(1641) => Ok(()),
        Some(1618) => bail!("Právě probíhá jiná instalace Windows. Počkejte na její dokončení a zkuste to znovu."),
        code => bail!(
            "Odinstalace předchozí verze GeoStore V6 selhala (kód {}). Podrobnosti: {}",
            code.unwrap_or(-1),
            log.display()
        ),
    }
}

pub fn msi_install(msi: &Path) -> Result<bool> {
    let log = msi_log("msi_install");
    let status = Command::new("msiexec.exe")
        .arg("/i")
        .arg(msi)
        .args(["/qn", "/norestart", "/l*v"])
        .arg(&log)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("Nelze spustit msiexec")?;
    match status.code() {
        Some(0) => Ok(false),
        Some(3010) | Some(1641) => Ok(true),
        Some(1618) => bail!("Právě probíhá jiná instalace Windows. Počkejte na její dokončení a zkuste to znovu."),
        code => bail!(
            "Instalace GeoStore V6 selhala (kód {}). Podrobnosti: {}",
            code.unwrap_or(-1),
            log.display()
        ),
    }
}

pub fn run_vcredist(mut log: impl FnMut(String)) {
    let redist = Path::new(paths::GEOSTORE_DIR).join("Redist");
    let items = [
        ("vc2010redist_x64.exe", vec!["/q", "/norestart"]),
        ("vc2012redist_x64.exe", vec!["/install", "/quiet", "/norestart"]),
    ];
    for (file, args) in items {
        let exe = redist.join(file);
        if !exe.is_file() {
            log(format!("{file} nebyl v {} nalezen.", redist.display()));
            continue;
        }
        match Command::new(&exe).args(&args).creation_flags(CREATE_NO_WINDOW).status() {
            Ok(s) => match s.code() {
                Some(0) | Some(3010) => log(format!("{file}: nainstalováno.")),
                Some(1638) | Some(5100) => log(format!("{file}: novější verze je již nainstalována.")),
                c => log(format!("{file}: instalátor skončil s kódem {}.", c.unwrap_or(-1))),
            },
            Err(e) => log(format!("{file}: nelze spustit ({e}).")),
        }
    }
}

pub fn find_hasp(root: &Path) -> Option<PathBuf> {
    let dir = root.join("Hasp");
    let win11 = dir.join("HASPUserSetup_windows_11.exe");
    let classic = dir.join("HASPUserSetup.exe");
    if local::windows_build() >= 22000 && win11.is_file() {
        Some(win11)
    } else if classic.is_file() {
        Some(classic)
    } else if win11.is_file() {
        Some(win11)
    } else {
        None
    }
}

pub fn run_interactive(exe: &Path) -> Result<i32> {
    let status = Command::new(exe)
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .status()
        .with_context(|| format!("Nelze spustit {}", exe.display()))?;
    Ok(status.code().unwrap_or(-1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robocopy_summary_is_parsed() {
        let mut o = DavkaOutcome::default();
        parse_robocopy("   Files :      1180      1180         0         0         2         0", &mut o);
        parse_robocopy("2026/11/10 20:56:35 ERROR 5 (0x00000005) Copying File x.dll", &mut o);
        assert_eq!(o.copied, 1180);
        assert_eq!(o.failed, 2);
        assert_eq!(o.errors, 1);
    }

    #[test]
    fn davka_is_made_unattended() {
        let dir = std::env::temp_dir().join(format!("gsu_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let src = b".\\GSV6\\setup.exe\r\nchcp 1250\r\ndel \"c:\\v6-konfig\\Adisplej\\DTM\xe8R\\x.xml\"\r\nrobocopy .\\DLL \"c:\\Program Files\\Geovap\\GeoStoreV6\" /E\r\n@echo off\r\nPAUSE\r\nset /p smaz=Vymazat adresar C:\\V6-INSTALL Y/N:\r\nif %smaz%==Y goto mazat\r\n";
        fs::write(dir.join("davka.bat"), src).unwrap();
        let out = fs::read(prepare_davka(&dir).unwrap()).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(!text.to_lowercase().contains("setup.exe"));
        assert!(!text.contains("PAUSE"));
        assert!(text.contains("set smaz=N"));
        assert!(out.windows(4).any(|w| w == b"DTM\xe8"));
        remove_dir(&dir);
    }
}
