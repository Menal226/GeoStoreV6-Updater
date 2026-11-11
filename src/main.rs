#![windows_subsystem = "windows"]

mod app;
mod catalog;
mod config;
mod creds;
mod daemon;
mod fmt;
mod installer;
mod local;
mod logfile;
mod logic;
mod paths;
mod portal;
mod scheduler;
mod selfupdate;
mod theme;
mod views;
mod widgets;
mod worker;

use eframe::egui;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;

const WINDOW_TITLE: &str = "GeoStore Updater";

fn acquire_instance_lock() -> std::io::Result<File> {
    let path = paths::data_dir().join("instance.lock");
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .share_mode(0)
        .open(path)
}

const ERROR_ACCESS_DENIED: i32 = 5;

fn focus_existing() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE};
    let title = local::wide(WINDOW_TITLE);
    unsafe {
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
            if !hwnd.is_invalid() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let flag = |name: &str| std::env::args().any(|a| a.eq_ignore_ascii_case(name));
    let relaunch = flag("--relaunch");
    if flag("--daemon") || flag("--background") {
        daemon::run(relaunch);
        return Ok(());
    }

    let mut lock = acquire_instance_lock();
    for _ in 0..(if relaunch { 50 } else { 0 }) {
        if lock.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        lock = acquire_instance_lock();
    }
    let _lock = match lock {
        Ok(file) => file,
        // Files an elevated run created in ProgramData are read-only for this unelevated one, so it asks for the same rights.
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) && !local::is_elevated() && !relaunch => {
            local::relaunch_elevated();
            return Ok(());
        }
        Err(_) => {
            focus_existing();
            return Ok(());
        }
    };

    let icon = eframe::icon_data::from_png_bytes(app::ICON_PNG).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, app::Launch { relaunch })))),
    )
}
