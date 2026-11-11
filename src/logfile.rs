use crate::paths;
use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warn,
    Error,
    Detail,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Success => "OK",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Detail => "DETAIL",
        }
    }

    fn from_tag(tag: &str) -> Self {
        match tag {
            "OK" => Level::Success,
            "WARN" => Level::Warn,
            "ERROR" => Level::Error,
            "DETAIL" => Level::Detail,
            _ => Level::Info,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: DateTime<Local>,
    pub level: Level,
    pub text: String,
}

static LOCK: Mutex<()> = Mutex::new(());

pub fn path() -> std::path::PathBuf {
    paths::logs_dir().join("updater.log")
}

pub fn append(line: &LogLine) {
    let _guard = LOCK.lock();
    let p = path();
    if let Ok(meta) = std::fs::metadata(&p) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = std::fs::rename(&p, p.with_extension("old.log"));
        }
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        for text in line.text.lines() {
            let _ = writeln!(
                f,
                "{} [{}] {}",
                line.time.format("%Y-%m-%d %H:%M:%S"),
                line.level.tag(),
                text
            );
        }
    }
}

pub fn read_tail(max: usize) -> Vec<LogLine> {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(max);
    lines[start..]
        .iter()
        .filter_map(|l| {
            let (ts, rest) = l.split_at_checked(19)?;
            let dt = NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S").ok()?;
            let time = Local.from_local_datetime(&dt).single()?;
            let rest = rest.trim_start().strip_prefix('[')?;
            let (tag, msg) = rest.split_once("] ")?;
            Some(LogLine {
                time,
                level: Level::from_tag(tag),
                text: msg.to_string(),
            })
        })
        .collect()
}

pub fn write(level: Level, text: &str) {
    append(&LogLine { time: Local::now(), level, text: text.to_string() });
}
