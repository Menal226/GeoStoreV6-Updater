use crate::catalog::ModuleInfo;
use crate::paths;
use chrono::{DateTime, Local, NaiveDate};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn wide_path(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub fn modified_date(p: &Path) -> Option<NaiveDate> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(DateTime::<Local>::from(m).date_naive())
}

pub fn file_version(path: &Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    if !path.is_file() {
        return None;
    }
    let w = wide_path(path);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(w.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _).ok()?;
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root = wide("\\");
        if !VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(root.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || ptr.is_null()
        {
            return None;
        }
        let info = &*(ptr as *const VS_FIXEDFILEINFO);
        Some(format!(
            "{}.{}.{}.{}",
            info.dwFileVersionMS >> 16,
            info.dwFileVersionMS & 0xffff,
            info.dwFileVersionLS >> 16,
            info.dwFileVersionLS & 0xffff
        ))
    }
}

pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|x| !x.is_empty())
            .map(|x| x.parse().unwrap_or(0))
            .collect()
    };
    let (mut va, mut vb) = (parse(a), parse(b));
    let n = va.len().max(vb.len());
    va.resize(n, 0);
    vb.resize(n, 0);
    va.cmp(&vb)
}

#[derive(Clone, Debug, Default)]
pub struct EditorInfo {
    pub installed: bool,
    pub display_version: Option<String>,
    pub product_codes: Vec<String>,
    pub install_date: Option<NaiveDate>,
    pub exe_version: Option<String>,
    pub exe_date: Option<NaiveDate>,
}

pub fn editor_info() -> EditorInfo {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    use winreg::RegKey;

    let mut info = EditorInfo::default();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let Ok(uninstall) = hklm.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            KEY_READ | view,
        ) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(key) = uninstall.open_subkey_with_flags(&name, KEY_READ | view) else {
                continue;
            };
            let display: String = key.get_value("DisplayName").unwrap_or_default();
            if display.trim().to_lowercase().replace(' ', "") != "geostorev6" {
                continue;
            }
            info.installed = true;
            if name.starts_with('{') && !info.product_codes.contains(&name) {
                info.product_codes.push(name.clone());
            }
            if let Ok(v) = key.get_value::<String, _>("DisplayVersion") {
                let newer = info
                    .display_version
                    .as_deref()
                    .map_or(true, |cur| compare_versions(&v, cur).is_gt());
                if newer {
                    info.display_version = Some(v);
                }
            }
            if let Ok(d) = key.get_value::<String, _>("InstallDate") {
                if let Ok(date) = NaiveDate::parse_from_str(d.trim(), "%Y%m%d") {
                    info.install_date = info.install_date.max(Some(date));
                }
            }
        }
    }
    let exe = Path::new(paths::GEOSTORE_DIR).join("GeoStore.exe");
    info.exe_version = file_version(&exe);
    info.exe_date = modified_date(&exe);
    if info.exe_version.is_some() {
        info.installed = true;
    }
    info
}

pub struct DavkaLog {
    tail: String,
    date: Option<NaiveDate>,
}

impl DavkaLog {
    pub fn read() -> Self {
        let path = paths::davka_log();
        let date = modified_date(&path);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).to_lowercase();
        let marks: Vec<usize> = text
            .match_indices("robust file copy")
            .map(|(i, _)| i)
            .collect();
        let start = if marks.len() >= 4 {
            marks[marks.len() - 4]
        } else {
            0
        };
        DavkaLog {
            tail: text[start..].to_string(),
            date,
        }
    }

    fn mentions(&self, file: &str) -> Option<NaiveDate> {
        if self.tail.contains(&file.to_lowercase()) {
            self.date
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LocalModule {
    pub detected: bool,
    pub version: Option<String>,
    pub evidence: Option<NaiveDate>,
}

pub fn detect_module(info: &ModuleInfo, davka: &DavkaLog) -> LocalModule {
    let dir = Path::new(paths::GEOSTORE_DIR);
    let found: Vec<PathBuf> = info
        .key_files
        .iter()
        .map(|f| dir.join(f))
        .filter(|p| p.is_file())
        .collect();
    let detected = !found.is_empty();
    let version = info.version_file.and_then(|f| file_version(&dir.join(f)));
    let mut evidence = found.iter().filter_map(|p| modified_date(p)).max();
    if detected {
        for f in &info.key_files {
            evidence = evidence.max(davka.mentions(f));
        }
    }
    LocalModule {
        detected,
        version,
        evidence,
    }
}

pub fn is_process_running(name: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if exe.eq_ignore_ascii_case(name) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

pub fn is_geostore_running() -> bool {
    is_process_running("GeoStore.exe")
}

pub fn msi_product_version(msi: &Path) -> Option<String> {
    use windows::Win32::System::ApplicationInstallationAndServicing::{
        MsiCloseHandle, MsiDatabaseOpenViewW, MsiOpenDatabaseW, MsiRecordGetStringW,
        MsiViewExecute, MsiViewFetch, MSIDBOPEN_READONLY, MSIHANDLE,
    };
    let path = wide_path(msi);
    let query = wide("SELECT `Value` FROM `Property` WHERE `Property`='ProductVersion'");
    let mut result = None;
    unsafe {
        let mut db = MSIHANDLE::default();
        if MsiOpenDatabaseW(PCWSTR(path.as_ptr()), MSIDBOPEN_READONLY, &mut db) != 0 {
            return None;
        }
        let mut view = MSIHANDLE::default();
        if MsiDatabaseOpenViewW(db, PCWSTR(query.as_ptr()), &mut view) == 0 {
            if MsiViewExecute(view, MSIHANDLE::default()) == 0 {
                let mut rec = MSIHANDLE::default();
                if MsiViewFetch(view, &mut rec) == 0 {
                    let mut buf = vec![0u16; 128];
                    let mut len = buf.len() as u32;
                    if MsiRecordGetStringW(rec, 1, PWSTR(buf.as_mut_ptr()), Some(&mut len)) == 0 {
                        result = Some(String::from_utf16_lossy(&buf[..len as usize]));
                    }
                    MsiCloseHandle(rec);
                }
            }
            MsiCloseHandle(view);
        }
        MsiCloseHandle(db);
    }
    result
}

pub fn windows_build() -> u32 {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

/// Starts a new copy of this program through the UAC prompt; false when the user declined or it failed.
pub fn relaunch_elevated() -> bool {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Some(exe) = crate::selfupdate::app_exe() else { return false };
    let exe = wide_path(&exe);
    let verb = wide("runas");
    let args = wide("--relaunch");
    let h = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(exe.as_ptr()), PCWSTR(args.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL) };
    h.0 as isize > 32
}

pub fn desktop_dir() -> PathBuf {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let raw: Option<String> = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
        .and_then(|k| k.get_value("Desktop"))
        .ok();
    match raw {
        Some(s) if !s.trim().is_empty() => PathBuf::from(expand_env(&s)),
        _ => PathBuf::from(expand_env(r"%USERPROFILE%\Desktop")),
    }
}

pub fn expand_env(s: &str) -> String {
    let re = regex::Regex::new(r"%([^%]+)%").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        std::env::var(&c[1]).unwrap_or_else(|_| c[0].to_string())
    })
    .into_owned()
}

pub fn open_in_explorer(path: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

/// A running program cannot delete itself, so a hidden cmd.exe waits for this process to exit and removes what is left.
pub fn remove_after_exit(files: &[PathBuf], dirs: &[PathBuf]) {
    use std::os::windows::process::CommandExt;
    let mut script = String::from("/c ping -n 4 127.0.0.1 >nul");
    for f in files {
        script.push_str(&format!(" & del /f /q \"{}\"", f.display()));
    }
    for d in dirs {
        script.push_str(&format!(" & rmdir /s /q \"{}\"", d.display()));
    }
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let _ = std::process::Command::new("cmd.exe")
        .raw_arg(script)
        .current_dir(root)
        .creation_flags(0x0800_0000)
        .spawn();
}

/// Going through explorer.exe keeps the browser out of this process's elevated token.
pub fn open_url(url: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
}

/// Light caption matching the sidebar, so the native title bar does not follow a dark system theme.
pub fn style_title_bar(title: &str) -> bool {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE};
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    let title = wide(title);
    unsafe {
        let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) else { return false };
        if hwnd.is_invalid() {
            return false;
        }
        let dark = 0i32;
        let caption = COLORREF(0x00F5_F4F4);
        let text = COLORREF(0x0017_1717);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &caption as *const _ as _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, &text as *const _ as _, 4);
    }
    true
}
