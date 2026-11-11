use crate::paths;
use chrono::{DateTime, Local, NaiveDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub type Manifest = BTreeMap<String, (u64, u64)>;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub auto_check: bool,
    pub check_mode: CheckMode,
    pub check_interval_hours: u32,
    pub daily_check_hour: u32,
    pub daily_check_minute: u32,
    pub auto_install: bool,
    pub backup_konfig: bool,
    pub keep_downloads: bool,
    pub remember_login: bool,
    pub last_user: String,
    pub tracked: BTreeMap<String, bool>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CheckMode {
    #[default]
    Interval,
    Daily,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_check: true,
            check_mode: CheckMode::Interval,
            check_interval_hours: 4,
            daily_check_hour: 8,
            daily_check_minute: 0,
            auto_install: false,
            backup_konfig: true,
            keep_downloads: false,
            remember_login: true,
            last_user: String::new(),
            tracked: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        load_json(&paths::data_dir().join("config.json"))
    }

    pub fn save(&self) {
        save_json(&paths::data_dir().join("config.json"), self);
    }

    pub fn is_tracked(&self, key: &str, detected: bool) -> bool {
        self.tracked.get(key).copied().unwrap_or(detected)
    }

    /// When the background check is due. A daily slot missed while the computer was off is caught up right away.
    pub fn next_check(&self, last: Option<DateTime<Local>>) -> DateTime<Local> {
        let now = Local::now();
        let Some(last) = last else { return DateTime::<chrono::Utc>::UNIX_EPOCH.with_timezone(&Local) };
        match self.check_mode {
            CheckMode::Interval => last + chrono::Duration::hours(self.check_interval_hours.max(1) as i64),
            CheckMode::Daily => {
                let slot = |date: NaiveDate| {
                    date.and_hms_opt(self.daily_check_hour.min(23), self.daily_check_minute.min(59), 0)
                        .and_then(|t| t.and_local_timezone(Local).earliest())
                        .unwrap_or(now)
                };
                let today = slot(now.date_naive());
                let previous = if now >= today { today } else { slot(now.date_naive() - chrono::Days::new(1)) };
                if last < previous {
                    previous
                } else if now < today {
                    today
                } else {
                    slot(now.date_naive() + chrono::Days::new(1))
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstalledRecord {
    pub portal_date: NaiveDate,
    pub record_id: String,
    pub file: String,
    pub installed_at: DateTime<Local>,
    pub version: Option<String>,
    pub complete: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct State {
    pub packages: BTreeMap<String, InstalledRecord>,
    pub last_check: Option<DateTime<Local>>,
    pub last_auto_check: Option<DateTime<Local>>,
    pub sizes: BTreeMap<String, u64>,
}

impl State {
    pub fn load() -> Self {
        load_json(&paths::data_dir().join("state.json"))
    }

    pub fn save(&self) {
        save_json(&paths::data_dir().join("state.json"), self);
    }

    /// The tray process and the window both write this file, so changes go onto the latest copy on disk.
    pub fn update(f: impl FnOnce(&mut State)) -> State {
        let mut state = Self::load();
        f(&mut state);
        state.save();
        state
    }
}

pub fn load_manifest(key: &str) -> Option<Manifest> {
    let path = paths::sub("manifests").join(format!("{key}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_manifest(key: &str, manifest: &Manifest) {
    save_json(&paths::sub("manifests").join(format!("{key}.json")), manifest);
}

fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_json<T: Serialize>(path: &Path, value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
