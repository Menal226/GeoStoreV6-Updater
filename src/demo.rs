//! Test account for store certification: it signs in without the portal, lists sample packages and only simulates installations, so nothing on the computer changes.

use crate::catalog::EDITOR;
use crate::portal::{Licence, RemoteFile};
use chrono::{Days, Local, NaiveDate};
use std::collections::BTreeMap;

const USER: &str = "test";
const PASS: &str = "test";

pub fn is_demo(user: &str, pass: &str) -> bool {
    user.trim().eq_ignore_ascii_case(USER) && pass == PASS
}

fn days_ago(n: u64) -> NaiveDate {
    Local::now().date_naive() - Days::new(n)
}

pub fn files() -> Vec<RemoteFile> {
    [
        ("zakladni_graficky_editor_GSV6_x64.zip", 10),
        ("castecna_instalace_aplikace_IG.zip", 5),
        ("kompletni_instalace_aplikace_IG_HW_klic_x64.zip", 40),
        ("castecna_instalace_aplikace_DTMCR.zip", 12),
        ("kompletni_instalace_aplikace_DTMCR_HW_klic_x64.zip", 50),
        ("castecna_instalace_aplikace_3D.zip", 20),
        ("kompletni_instalace_aplikace_3DLAS_HW_klic_x64.zip", 45),
        ("instalace_rozsireni_GSV6_DWG_DGN_x64.zip", 30),
    ]
    .iter()
    .enumerate()
    .map(|(i, (title, age))| RemoteFile {
        id: format!("demo-{i}"),
        title: title.to_string(),
        date: days_ago(*age),
        code: String::new(),
        role: String::new(),
        folder: String::new(),
    })
    .collect()
}

pub fn licences() -> Vec<Licence> {
    vec![
        Licence::sample("00000000-0000-0000-0000-000000000001", "GeoStore V6", "3", "1", ""),
        Licence::sample("00000000-0000-0000-0000-000000000002", "GeoStore V6 – V6-IG", "1", "0", "31.12.2027"),
    ]
}

/// What the simulated computer has installed: version and file date per package.
pub struct Demo {
    installed: BTreeMap<String, (String, NaiveDate)>,
}

impl Demo {
    pub fn new() -> Self {
        let installed = [(EDITOR, "6.3.11.0", 60), ("IG", "6.3.12.0", 5), ("DTMCR", "6.3.10.0", 90)]
            .into_iter()
            .map(|(key, version, age)| (key.to_string(), (version.to_string(), days_ago(age))))
            .collect();
        Self { installed }
    }

    pub fn detect(&self, key: &str) -> (bool, Option<String>, Option<NaiveDate>) {
        match self.installed.get(key) {
            Some((version, date)) => (true, Some(version.clone()), Some(*date)),
            None => (false, None, None),
        }
    }

    pub fn install(&mut self, key: &str, date: NaiveDate) {
        self.installed.insert(key.to_string(), ("6.3.12.0".to_string(), date));
    }
}
