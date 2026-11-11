use crate::config::{Config, State};
use crate::logfile::{self, Level};
use crate::logic::{self, PackageRow};
use crate::worker::{self, Cmd, Event, InstallOptions};
use crate::{creds, fmt, local, paths, selfupdate};
use chrono::Local;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, LoadImageW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, ASFW_ANY, HICON, IMAGE_ICON,
    LR_DEFAULTCOLOR, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXICON, SM_CXSMICON, SYSTEM_METRICS_INDEX, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WNDCLASSW,
    WS_OVERLAPPED,
};

const WM_TRAY: u32 = WM_APP + 1;
const WM_FLUSH: u32 = WM_APP + 2;
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const ID_OPEN: usize = 1;
const ID_CHECK: usize = 2;
const ID_EXIT: usize = 3;
const EVALUATE_EVERY: Duration = Duration::from_secs(30);
const RETRY_AFTER: Duration = Duration::from_secs(15 * 60);
const SELF_UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);

/// State the scheduler thread hands to the tray window, which owns every Shell_NotifyIcon call.
struct Shared {
    hwnd: AtomicIsize,
    tip: Mutex<String>,
    toasts: Mutex<Vec<(String, String)>>,
    installing: AtomicBool,
    check_now: AtomicBool,
}

static SHARED: Shared = Shared {
    hwnd: AtomicIsize::new(0),
    tip: Mutex::new(String::new()),
    toasts: Mutex::new(Vec::new()),
    installing: AtomicBool::new(false),
    check_now: AtomicBool::new(false),
};
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn lock_file() -> Option<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(paths::data_dir().join("daemon.lock"))
        .ok()
}

pub fn ensure_running() {
    if lock_file().is_none() {
        return;
    }
    if let Some(exe) = selfupdate::app_exe() {
        let _ = Command::new(exe).arg("--daemon").spawn();
    }
}

pub fn run(relaunch: bool) {
    let mut lock = lock_file();
    for _ in 0..(if relaunch { 50 } else { 0 }) {
        if lock.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        lock = lock_file();
    }
    let Some(_lock) = lock else { return };

    let Some(hwnd) = (unsafe { create_window() }) else { return };
    SHARED.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
    set_tip(status_tip(&Config::load(), "", false));
    for _ in 0..60 {
        if unsafe { add_icon(hwnd) } {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    std::thread::Builder::new().name("scheduler".into()).spawn(scheduler).expect("scheduler thread");

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn post(msg: u32) {
    let hwnd = HWND(SHARED.hwnd.load(Ordering::SeqCst) as *mut _);
    unsafe {
        let _ = PostMessageW(hwnd, msg, WPARAM(0), LPARAM(0));
    }
}

fn set_tip(text: String) {
    *SHARED.tip.lock().unwrap() = text;
    post(WM_FLUSH);
}

fn toast(title: impl Into<String>, text: impl Into<String>) {
    SHARED.toasts.lock().unwrap().push((title.into(), text.into()));
    post(WM_FLUSH);
}

fn status_tip(cfg: &Config, activity: &str, no_creds: bool) -> String {
    let line = if !activity.is_empty() {
        activity.to_string()
    } else if !cfg.auto_check {
        "Automatická kontrola je vypnutá".to_string()
    } else if no_creds {
        "Pro automatickou kontrolu uložte přihlášení".to_string()
    } else {
        let next = cfg.next_check(State::load().last_auto_check);
        if next <= Local::now() {
            "Kontrola proběhne za chvíli".to_string()
        } else {
            format!("Další kontrola {}", fmt::datetime(next))
        }
    };
    format!("GeoStore Updater\n{line}")
}

fn open_window() {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Some(exe) = selfupdate::app_exe() else { return };
    let exe = local::wide_path(&exe);
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(None, w!("open"), PCWSTR(exe.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn scheduler() {
    let mut next_self_update = Instant::now();
    let mut last_evaluated: Option<Instant> = None;
    let mut retry_at: Option<Instant> = None;
    let mut announced: Vec<String> = Vec::new();
    loop {
        if Instant::now() >= next_self_update {
            next_self_update = Instant::now() + SELF_UPDATE_EVERY;
            if selfupdate::run() {
                if let Some(exe) = selfupdate::app_exe() {
                    let _ = Command::new(exe).args(["--daemon", "--relaunch"]).spawn();
                }
                post(WM_CLOSE);
                return;
            }
        }
        let forced = SHARED.check_now.swap(false, Ordering::SeqCst);
        if forced || last_evaluated.is_none_or(|t| t.elapsed() >= EVALUATE_EVERY) {
            last_evaluated = Some(Instant::now());
            let cfg = Config::load();
            let no_creds = !creds::exists();
            let due = cfg.auto_check
                && !no_creds
                && Local::now() >= cfg.next_check(State::load().last_auto_check)
                && retry_at.is_none_or(|t| Instant::now() >= t);
            if forced && no_creds {
                toast("Kontrola aktualizací není možná", "Otevřete GeoStore Updater a uložte přihlášení k portálu.");
            } else if forced || due {
                match check(&cfg, forced, &mut announced) {
                    Ok(()) => retry_at = None,
                    Err(e) => {
                        retry_at = Some(Instant::now() + RETRY_AFTER);
                        if forced {
                            toast("Kontrola aktualizací selhala", e);
                        }
                    }
                }
            }
            set_tip(status_tip(&cfg, "", no_creds));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn check(cfg: &Config, forced: bool, announced: &mut Vec<String>) -> Result<(), String> {
    let Some((user, pass)) = creds::load() else { return Ok(()) };
    set_tip(status_tip(cfg, "Kontroluji aktualizace…", false));
    let (tx, rx, _cancel) = worker::spawn(None);
    let _ = tx.send(Cmd::Login { user, pass, remember: true });
    let rows = loop {
        match rx.recv_timeout(Duration::from_secs(600)) {
            Ok(Event::Overview { rows, .. }) => break rows,
            Ok(Event::LoginFailed(e)) | Ok(Event::CheckFailed(e)) => return Err(e),
            Ok(_) => {}
            Err(_) => {
                let msg = "Portál neodpověděl včas.".to_string();
                logfile::write(Level::Error, &format!("Automatická kontrola selhala: {msg}"));
                return Err(msg);
            }
        }
    };
    State::update(|s| s.last_auto_check = Some(Local::now()));

    let pending = logic::pending_keys(&rows, cfg);
    if pending.is_empty() {
        announced.clear();
        if forced {
            toast("GeoStore V6 je aktuální", "Všechny sledované balíčky jsou aktuální.");
        }
        return Ok(());
    }
    let names = names(&rows, &pending);
    if cfg.auto_install && local::is_elevated() {
        announced.clear();
        install(cfg, &rows, pending, &names, &tx, &rx);
        return Ok(());
    }
    let signature: Vec<String> = pending
        .iter()
        .map(|k| {
            let date = rows.iter().find(|r| &r.key == k).and_then(|r| r.latest.as_ref()).map(|f| f.date.to_string());
            format!("{k}@{}", date.unwrap_or_default())
        })
        .collect();
    if forced || signature != *announced {
        *announced = signature;
        let title = match pending.len() {
            1 => "K dispozici je aktualizace GeoStore V6".to_string(),
            n @ 2..=4 => format!("K dispozici jsou {n} aktualizace GeoStore V6"),
            n => format!("K dispozici je {n} aktualizací GeoStore V6"),
        };
        toast(title, format!("{names}\nKliknutím otevřete GeoStore Updater."));
    }
    Ok(())
}

fn names(rows: &[PackageRow], keys: &[String]) -> String {
    keys.iter()
        .map(|k| rows.iter().find(|r| &r.key == k).map(|r| r.info.name.clone()).unwrap_or_else(|| k.clone()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn install(cfg: &Config, rows: &[PackageRow], keys: Vec<String>, names: &str, tx: &std::sync::mpsc::Sender<Cmd>, rx: &Receiver<Event>) {
    SHARED.installing.store(true, Ordering::SeqCst);
    set_tip(status_tip(cfg, "Instaluji aktualizace…", false));
    let opts = InstallOptions {
        backup: cfg.backup_konfig,
        keep_downloads: cfg.keep_downloads,
        tracked: rows.iter().filter(|r| cfg.is_tracked(&r.key, r.detected)).map(|r| r.key.clone()).collect(),
        force: false,
    };
    let _ = tx.send(Cmd::Install { keys, opts });
    let mut warned = false;
    while let Ok(ev) = rx.recv() {
        match ev {
            Event::WaitingForGeoStore(true) if !warned => {
                warned = true;
                set_tip(status_tip(cfg, "Instalace čeká na ukončení GeoStore V6", false));
                toast("Aktualizace čekají na ukončení GeoStore V6", "Uložte práci a zavřete GeoStore V6. Instalace potom automaticky pokračuje.");
            }
            Event::OpFinished { ok, summary } => {
                if ok {
                    toast("Aktualizace nainstalovány", names);
                } else if summary.starts_with("Instalace byla dokončena") {
                    toast("Aktualizace nainstalovány s upozorněním", "Podrobnosti najdete v historii aplikace GeoStore Updater.");
                } else {
                    toast("Instalace aktualizací se nezdařila", summary);
                }
                break;
            }
            _ => {}
        }
    }
    SHARED.installing.store(false, Ordering::SeqCst);
}

fn copy_wide(dst: &mut [u16], text: &str) {
    let src: Vec<u16> = text.encode_utf16().take(dst.len() - 1).collect();
    dst[..src.len()].copy_from_slice(&src);
    dst[src.len()] = 0;
}

unsafe fn instance() -> HINSTANCE {
    GetModuleHandleW(PCWSTR::null()).map(Into::into).unwrap_or_default()
}

unsafe fn load_icon(metric: SYSTEM_METRICS_INDEX) -> HICON {
    let size = GetSystemMetrics(metric);
    LoadImageW(instance(), PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
        .map(|h| HICON(h.0))
        .unwrap_or_default()
}

unsafe fn create_window() -> Option<HWND> {
    let class = w!("GeoStoreUpdaterTray");
    let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance(), lpszClassName: class, ..Default::default() };
    RegisterClassW(&wc);
    TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
    CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR::null(), WS_OVERLAPPED, 0, 0, 0, 0, None, None, instance(), None).ok()
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

unsafe fn add_icon(hwnd: HWND) -> bool {
    let mut d = icon_data(hwnd);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = load_icon(SM_CXSMICON);
    copy_wide(&mut d.szTip, &SHARED.tip.lock().unwrap());
    if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
        return false;
    }
    d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
    true
}

unsafe fn flush(hwnd: HWND) {
    let mut d = icon_data(hwnd);
    d.uFlags = NIF_TIP | NIF_SHOWTIP;
    copy_wide(&mut d.szTip, &SHARED.tip.lock().unwrap());
    let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    let toasts = std::mem::take(&mut *SHARED.toasts.lock().unwrap());
    for (title, text) in toasts {
        let mut d = icon_data(hwnd);
        d.uFlags = NIF_INFO;
        copy_wide(&mut d.szInfoTitle, &title);
        copy_wide(&mut d.szInfo, &text);
        d.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
        d.hBalloonIcon = load_icon(SM_CXICON);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let installing = SHARED.installing.load(Ordering::SeqCst);
    let busy = if installing { MF_STRING | MF_GRAYED } else { MF_STRING };
    let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("Otevřít GeoStore Updater"));
    let _ = AppendMenuW(menu, busy, ID_CHECK, w!("Zkontrolovat aktualizace"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, busy, ID_EXIT, w!("Ukončit"));
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD, pt.x, pt.y, 0, hwnd, None).0 as usize;
    let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    match cmd {
        ID_OPEN => open_window(),
        ID_CHECK => SHARED.check_now.store(true, Ordering::SeqCst),
        ID_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                NIN_SELECT | NIN_KEYSELECT | NIN_BALLOONUSERCLICK => open_window(),
                WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_FLUSH => {
            flush(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            add_icon(hwnd);
            flush(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
