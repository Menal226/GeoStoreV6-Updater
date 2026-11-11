use crate::catalog::{self, ModuleInfo, Variant, EDITOR};
use crate::config::{Config, State};
use crate::local::{self, DavkaLog};
use crate::portal::RemoteFile;
use chrono::{Duration, NaiveDate};
use std::collections::BTreeMap;

// Timestamps copied from a package are a few days older than its portal date, so the
// first-run estimate (before the updater has installed anything itself) allows some slack.
const FIRST_RUN_TOLERANCE_DAYS: i64 = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    UpToDate,
    UpdateAvailable,
    NotInstalled,
}

#[derive(Clone, Debug)]
pub struct PackageRow {
    pub key: String,
    pub info: ModuleInfo,
    pub detected: bool,
    pub local_version: Option<String>,
    pub installed_date: Option<NaiveDate>,
    pub from_state: bool,
    pub latest: Option<RemoteFile>,
    pub complete: Option<RemoteFile>,
    pub status: Status,
}

impl PackageRow {
    pub fn is_editor(&self) -> bool {
        self.key == EDITOR
    }
}

#[derive(Default)]
struct Group {
    partial: Option<RemoteFile>,
    complete: Option<RemoteFile>,
    single: Option<RemoteFile>,
}

fn keep_newest(slot: &mut Option<RemoteFile>, f: &RemoteFile) {
    if slot.as_ref().map_or(true, |cur| f.date > cur.date) {
        *slot = Some(f.clone());
    }
}

pub fn compute_rows(files: &[RemoteFile], state: &State) -> Vec<PackageRow> {
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for f in files {
        if let Some((key, variant)) = catalog::classify(&f.title) {
            let g = groups.entry(key).or_default();
            match variant {
                Variant::Partial => keep_newest(&mut g.partial, f),
                Variant::Complete => keep_newest(&mut g.complete, f),
                Variant::Single => keep_newest(&mut g.single, f),
            }
        }
    }

    let davka = DavkaLog::read();
    let editor = local::editor_info();
    let mut rows = Vec::new();
    for (key, g) in groups {
        let info = catalog::module_info(&key);
        let latest = g.partial.clone().or_else(|| g.single.clone()).or_else(|| g.complete.clone());
        let complete = g.complete.or(g.single);
        let (detected, local_version, evidence) = if key == EDITOR {
            (
                editor.installed,
                editor.exe_version.clone().or_else(|| editor.display_version.clone()),
                editor.install_date.max(editor.exe_date),
            )
        } else {
            let m = local::detect_module(&info, &davka);
            (m.detected, m.version, m.evidence)
        };
        let record = state.packages.get(&key).filter(|_| detected);
        let (installed_date, from_state) = match record {
            Some(r) => (Some(r.portal_date), true),
            None => (evidence, false),
        };
        let status = if !detected {
            Status::NotInstalled
        } else {
            match (&latest, installed_date) {
                (Some(l), Some(d)) => {
                    let threshold = if from_state {
                        l.date
                    } else {
                        l.date - Duration::days(FIRST_RUN_TOLERANCE_DAYS)
                    };
                    if d >= threshold {
                        Status::UpToDate
                    } else {
                        Status::UpdateAvailable
                    }
                }
                (Some(_), None) => Status::UpdateAvailable,
                (None, _) => Status::UpToDate,
            }
        };
        rows.push(PackageRow {
            key,
            info,
            detected,
            local_version,
            installed_date,
            from_state,
            latest,
            complete,
            status,
        });
    }
    rows.sort_by(|a, b| a.info.order.cmp(&b.info.order).then(a.key.cmp(&b.key)));
    rows
}

pub fn pending_keys(rows: &[PackageRow], cfg: &Config) -> Vec<String> {
    rows.iter()
        .filter(|r| cfg.is_tracked(&r.key, r.detected) && r.status == Status::UpdateAvailable)
        .map(|r| r.key.clone())
        .collect()
}
