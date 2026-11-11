use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chrono::NaiveDate;
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const BASE: &str = "https://portal.geostore.cz/v6/support/framework/";
pub const LICMAN: &str = "https://install.geovap.cz/licman/app/framework/";
const API_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub struct SessionExpired;

impl std::fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Platnost přihlášení na portálu vypršela.")
    }
}

impl std::error::Error for SessionExpired {}

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operace byla zrušena.")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Cancelled>().is_some()
}

fn is_session_expired(e: &anyhow::Error) -> bool {
    e.downcast_ref::<SessionExpired>().is_some()
}

#[derive(Clone, Debug, Deserialize)]
pub struct Role {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteFile {
    pub id: String,
    pub title: String,
    pub date: NaiveDate,
    pub code: String,
    pub role: String,
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Licence {
    pub guid: String,
    pub project: String,
    pub company: String,
    pub float_max: String,
    pub float_used: String,
    pub valid_to: String,
    pub trial: bool,
    code: String,
}

impl Licence {
    pub fn title(&self) -> String {
        if self.project.is_empty() { "Licence GeoStore V6".into() } else { self.project.clone() }
    }

    pub fn detail(&self) -> String {
        let mut parts = Vec::new();
        if !self.company.is_empty() {
            parts.push(self.company.clone());
        }
        if !self.float_max.is_empty() {
            parts.push(format!("plovoucí {} / {}", if self.float_used.is_empty() { "0" } else { &self.float_used }, self.float_max));
        }
        parts.push(match self.valid_to.split_whitespace().next() {
            Some(d) => format!("platnost do {d}"),
            None => "bez omezení platnosti".into(),
        });
        if self.trial {
            parts.push("zkušební".into());
        }
        parts.join(" · ")
    }
}

pub struct Portal {
    client: Client,
    base: &'static str,
    creds: Option<(String, String)>,
    pub user_name: String,
    pub roles: Vec<Role>,
}

impl Portal {
    pub fn new() -> Result<Self> {
        Self::with_base(BASE)
    }

    pub fn licman() -> Result<Self> {
        Self::with_base(LICMAN)
    }

    fn with_base(base: &'static str) -> Result<Self> {
        let client = Client::builder()
            .cookie_store(true)
            .user_agent(concat!("GeoStoreUpdater/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(None::<Duration>)
            .build()?;
        Ok(Self {
            client,
            base,
            creds: None,
            user_name: String::new(),
            roles: Vec::new(),
        })
    }

    pub fn credentials(&self) -> Option<(String, String)> {
        self.creds.clone()
    }

    pub fn list_licences(&mut self) -> Result<Vec<Licence>> {
        let shape = licence_shape();
        let mut out: Vec<Licence> = Vec::new();
        for role in self.roles.clone() {
            let v = self.call(json!({
                "action": "mobile.role",
                "user_seat": "",
                "user_role": role.name,
                "folder_id": ""
            }))?;
            let mut folders = Vec::new();
            collect_all_folders(v.get("folders"), &mut folders);
            for (folder, sql) in folders {
                if !sql.contains("licen") {
                    continue;
                }
                let v = self.call(json!({
                    "action": "mobile.review",
                    "user_seat": "",
                    "user_role": role.name,
                    "folder_id": folder,
                    "parameters": [
                        {"name": "page", "type": "N", "value": 1},
                        {"name": "count", "type": "N", "value": 200}
                    ]
                }))?;
                let data = &v["table"]["data"];
                let cols: Vec<String> = data["columns"]
                    .as_array()
                    .map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_lowercase()).collect())
                    .unwrap_or_default();
                let idx = |n: &str| cols.iter().position(|c| c == n);
                let (Some(i_code), Some(i_key)) = (idx("code"), idx("floating_guid")) else {
                    continue;
                };
                for row in data["rows"].as_array().cloned().unwrap_or_default() {
                    let get = |name: &str| -> String {
                        match idx(name).and_then(|i| row.get(i)) {
                            Some(Value::String(s)) => s.trim().to_string(),
                            Some(Value::Null) | None => String::new(),
                            Some(o) => o.to_string(),
                        }
                    };
                    let prod = get("prod_name");
                    if !prod.is_empty() && !prod.to_uppercase().contains("GSV6") {
                        continue;
                    }
                    let key = row.get(i_key).and_then(Value::as_str).unwrap_or("").trim().to_string();
                    if !shape.is_match(&key) {
                        continue;
                    }
                    let guid = key.split('|').next().unwrap_or_default().to_lowercase();
                    if out.iter().any(|l| l.guid == guid) {
                        continue;
                    }
                    let trial = get("trial").eq_ignore_ascii_case("true") || get("trial_desc").eq_ignore_ascii_case("ano");
                    out.push(Licence {
                        guid,
                        project: html_escape::decode_html_entities(&get("project")).to_string(),
                        company: html_escape::decode_html_entities(&get("companyname")).to_string(),
                        float_max: get("float_lic_max_count"),
                        float_used: get("used_float_lic_count"),
                        valid_to: get("valid_to"),
                        trial,
                        code: row.get(i_code).and_then(Value::as_str).unwrap_or("").to_string(),
                    });
                }
            }
        }
        out.sort_by_key(|l| l.trial);
        Ok(out)
    }

    pub fn download_licence(&mut self, guid: &str) -> Result<String> {
        let guid = guid.to_lowercase();
        let Some(lic) = self.list_licences()?.into_iter().find(|l| l.guid == guid) else {
            bail!("Vybraná licence už v systému licman není k dispozici.");
        };
        let params = json!([{"name": "selection$code", "type": "PKC", "value": lic.code}]);
        let b64 = base64::engine::general_purpose::STANDARD.encode(params.to_string());
        let resp = self
            .client
            .get(format!("{}AsyncJSON.ashx", self.base))
            .query(&[
                ("action", "databaseFileDownload"),
                ("resourceId", "resDownloadLicFile"),
                ("output", "gsfl.lic"),
                ("params", b64.as_str()),
            ])
            .timeout(API_TIMEOUT)
            .send()
            .context("Nelze se spojit se serverem install.geovap.cz")?;
        if !resp.status().is_success() {
            bail!("Licman při stahování licence vrátil chybu HTTP {}.", resp.status());
        }
        let text = resp.text()?.trim().to_string();
        if !licence_shape().is_match(&text) || !text.to_lowercase().starts_with(&guid) {
            bail!("Licman nevydal platný licenční soubor.");
        }
        Ok(text)
    }

    pub fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        let body = json!({
            "action": "mobile.authentication",
            "parameters": [
                {"name": "action", "value": "mobile.authentication"},
                {"name": "identifier", "value": ""},
                {"name": "user_name", "value": user},
                {"name": "user_password", "value": pass}
            ]
        });
        let resp = self
            .client
            .post(format!("{}AsyncJSON.ashx", self.base))
            .json(&body)
            .timeout(API_TIMEOUT)
            .send()
            .context("Nelze se spojit s portálem portal.geostore.cz")?;
        match resp.status().as_u16() {
            200 => {}
            403 => bail!("Pokus o přihlášení byl zamítnut, pravděpodobně kvůli příliš mnoha neúspěšným pokusům. Zkuste to prosím později."),
            429 => bail!("Portál dočasně omezil počet požadavků. Zkuste to prosím za chvíli."),
            s => bail!("Portál při přihlášení vrátil chybu HTTP {s}."),
        }
        let v: Value = resp
            .json()
            .context("Portál vrátil neočekávanou odpověď při přihlášení")?;
        if let Some(m) = v.get("message").and_then(Value::as_str) {
            bail!("{m}");
        }
        if v.get("users").is_some() {
            bail!("Účet je přiřazen k více uživatelům. Tento typ účtu zatím není podporován.");
        }
        let state = state_of(&v);
        if state >= 2 || v.get("url").is_none() {
            let msg = v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Přihlášení se nezdařilo. Zkontrolujte jméno a heslo.");
            bail!("{msg}");
        }
        self.user_name = v
            .get("user_name")
            .and_then(Value::as_str)
            .unwrap_or(user)
            .to_string();
        self.creds = Some((user.to_string(), pass.to_string()));
        self.load_roles()
    }

    fn relogin(&mut self) -> Result<()> {
        let (u, p) = self
            .creds
            .clone()
            .ok_or_else(|| anyhow!("Nejste přihlášeni k portálu."))?;
        self.login(&u, &p)
    }

    fn load_roles(&mut self) -> Result<()> {
        let resp = self
            .client
            .get(format!("{}Overview.aspx", self.base))
            .timeout(API_TIMEOUT)
            .send()?;
        if resp.url().as_str().to_lowercase().contains("login.html") {
            return Err(SessionExpired.into());
        }
        let html = resp.text()?;
        let re = Regex::new(r#""availableRoles":(\[[^\]]*\])"#).unwrap();
        let mut roles: Vec<Role> = re
            .captures(&html)
            .and_then(|c| serde_json::from_str(&c[1]).ok())
            .unwrap_or_default();
        if roles.is_empty() {
            let re = Regex::new(r#""role":"([^"]+)","roleTitle":""#).unwrap();
            if let Some(c) = re.captures(&html) {
                roles.push(Role {
                    name: c[1].to_string(),
                });
            }
        }
        if roles.is_empty() {
            bail!("K účtu nejsou na portálu přiřazeny žádné licence.");
        }
        self.roles = roles;
        Ok(())
    }

    fn api(&self, body: &Value) -> Result<Value> {
        let resp = self
            .client
            .post(format!("{}AsyncJSON.ashx", self.base))
            .json(body)
            .timeout(API_TIMEOUT)
            .send()
            .context("Nelze se spojit s portálem portal.geostore.cz")?;
        match resp.status().as_u16() {
            200 => {}
            401 => return Err(SessionExpired.into()),
            s => bail!("Portál vrátil chybu HTTP {s}."),
        }
        let path = resp.url().path().to_lowercase();
        if path.contains("login") || path.contains("logout") {
            return Err(SessionExpired.into());
        }
        let v: Value = resp.json().context("Portál vrátil neočekávanou odpověď")?;
        let state = state_of(&v);
        if state == 99 {
            return Err(SessionExpired.into());
        }
        if state > 1 {
            let m = v
                .get("message")
                .or_else(|| v.get("error"))
                .and_then(Value::as_str)
                .unwrap_or("neznámá chyba");
            bail!("Portál vrátil chybu: {m}");
        }
        Ok(v)
    }

    fn call(&mut self, body: Value) -> Result<Value> {
        match self.api(&body) {
            Err(e) if is_session_expired(&e) => {
                self.relogin()?;
                self.api(&body)
            }
            r => r,
        }
    }

    pub fn list_files(&mut self) -> Result<Vec<RemoteFile>> {
        if self.roles.is_empty() {
            match self.load_roles() {
                Err(e) if is_session_expired(&e) => self.relogin()?,
                r => r?,
            }
        }
        let mut out: Vec<RemoteFile> = Vec::new();
        for role in self.roles.clone() {
            let v = self.call(json!({
                "action": "mobile.role",
                "user_seat": "",
                "user_role": role.name,
                "folder_id": ""
            }))?;
            let mut folders = Vec::new();
            collect_folders(v.get("folders"), &mut folders);
            for folder in folders {
                for f in self.review(&role.name, &folder)? {
                    if !out.iter().any(|o| o.id == f.id) {
                        out.push(f);
                    }
                }
            }
        }
        Ok(out)
    }

    fn review(&mut self, role: &str, folder: &str) -> Result<Vec<RemoteFile>> {
        let mut out = Vec::new();
        let mut page = 1i64;
        loop {
            let v = self.call(json!({
                "action": "mobile.review",
                "user_seat": "",
                "user_role": role,
                "folder_id": folder,
                "parameters": [
                    {"name": "page", "type": "N", "value": page},
                    {"name": "count", "type": "N", "value": 200}
                ]
            }))?;
            let data = &v["table"]["data"];
            let cols: Vec<String> = data["columns"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|c| c.as_str().unwrap_or("").to_lowercase())
                        .collect()
                })
                .unwrap_or_default();
            let idx = |n: &str| cols.iter().position(|c| c == n);
            let (Some(i_id), Some(i_title), Some(i_date), Some(i_code)) =
                (idx("id"), idx("web_title"), idx("datum"), idx("code"))
            else {
                return Ok(out);
            };
            for row in data["rows"].as_array().cloned().unwrap_or_default() {
                let get = |i: usize| -> String {
                    match row.get(i) {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Null) | None => String::new(),
                        Some(o) => o.to_string(),
                    }
                };
                let Some(date) = parse_date(&get(i_date)) else {
                    continue;
                };
                out.push(RemoteFile {
                    id: get(i_id),
                    title: html_escape::decode_html_entities(&get(i_title))
                        .trim()
                        .to_string(),
                    date,
                    code: get(i_code),
                    role: role.to_string(),
                    folder: folder.to_string(),
                });
            }
            let pages = data["pages"].as_i64().unwrap_or(1);
            if page >= pages || page >= 50 {
                break;
            }
            page += 1;
        }
        Ok(out)
    }

    pub fn resolve_download(&mut self, f: &RemoteFile) -> Result<String> {
        let pars = json!({"pars": [
            ["user_role", f.role, ""],
            ["code", f.code, "pkc"],
            ["folder_id", f.folder, ""],
            ["detail", "tabDetailAkce", ""],
            ["window", "frmFileDetail", ""],
            ["select", "rsDetailAkce", ""]
        ]});
        let b64 = base64::engine::general_purpose::STANDARD.encode(pars.to_string());
        let re = Regex::new(r#"href="(AsyncJSON\.ashx\?action=repositoryFileDownload[^"]*)""#)
            .unwrap();
        for attempt in 0..2 {
            let html = self
                .client
                .post(format!("{}Dialog.aspx", self.base))
                .form(&[("pars", b64.as_str())])
                .timeout(API_TIMEOUT)
                .send()
                .context("Nelze se spojit s portálem portal.geostore.cz")?
                .text()?;
            if let Some(c) = re.captures(&html) {
                let link = html_escape::decode_html_entities(&c[1]).to_string();
                return Ok(format!("{}{link}", self.base));
            }
            if attempt == 0 {
                self.relogin()?;
            }
        }
        bail!(
            "Na portálu se nepodařilo najít odkaz ke stažení souboru {}.",
            f.title
        )
    }

    pub fn download(
        &mut self,
        url: &str,
        dest: &Path,
        cancel: &AtomicBool,
        mut on_progress: impl FnMut(u64),
    ) -> Result<u64> {
        let mut attempt = 0;
        let mut resp = loop {
            let resp = self
                .client
                .get(url)
                .send()
                .context("Stahování se nezdařilo – nelze se spojit se serverem")?;
            if !resp.status().is_success() {
                bail!("Server při stahování vrátil chybu HTTP {}.", resp.status());
            }
            let ct = resp
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_lowercase();
            if ct.contains("json") || ct.contains("text/html") {
                let text = resp.text().unwrap_or_default();
                if attempt == 0 {
                    attempt += 1;
                    self.relogin()?;
                    continue;
                }
                let short: String = text.chars().take(200).collect();
                bail!("Portál soubor nevydal: {short}");
            }
            break resp;
        };

        let part = dest.with_extension("part");
        let mut file = BufWriter::with_capacity(1 << 20, File::create(&part)?);
        let mut buf = vec![0u8; 256 * 1024];
        let mut total = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(Cancelled.into());
            }
            let n = match resp.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    drop(file);
                    let _ = std::fs::remove_file(&part);
                    return Err(anyhow!(e).context("Stahování bylo přerušeno"));
                }
            };
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            total += n as u64;
            on_progress(total);
        }
        file.flush()?;
        drop(file);
        if total == 0 {
            let _ = std::fs::remove_file(&part);
            bail!("Stažený soubor je prázdný.");
        }
        std::fs::rename(&part, dest)?;
        Ok(total)
    }
}

fn state_of(v: &Value) -> i64 {
    v.get("state")
        .and_then(|s| s.as_i64().or_else(|| s.as_str().and_then(|x| x.parse().ok())))
        .unwrap_or(1)
}

fn collect_folders(v: Option<&Value>, out: &mut Vec<String>) {
    let Some(arr) = v.and_then(Value::as_array) else {
        return;
    };
    for f in arr {
        let id = f
            .get("clientId")
            .or_else(|| f.get("name"))
            .and_then(Value::as_str);
        let sql = f.get("sql").and_then(Value::as_str).unwrap_or("").to_lowercase();
        if let Some(id) = id {
            if sql.is_empty() || sql.contains("file") {
                out.push(id.to_string());
            }
        }
        collect_folders(f.get("childs"), out);
    }
}

fn licence_shape() -> Regex {
    Regex::new(r"^[0-9A-Fa-f-]{36}\|[A-Za-z0-9+/]+=*$").unwrap()
}

fn collect_all_folders(v: Option<&Value>, out: &mut Vec<(String, String)>) {
    let Some(arr) = v.and_then(Value::as_array) else {
        return;
    };
    for f in arr {
        let id = f.get("clientId").or_else(|| f.get("name")).and_then(Value::as_str);
        let sql = f.get("sql").and_then(Value::as_str).unwrap_or("").to_lowercase();
        if let Some(id) = id {
            out.push((id.to_string(), sql));
        }
        collect_all_folders(f.get("childs"), out);
    }
}

pub fn parse_date(s: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = s
        .trim()
        .split(['.', ' '])
        .filter(|x| !x.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    NaiveDate::from_ymd_opt(
        parts[2].parse().ok()?,
        parts[1].parse().ok()?,
        parts[0].parse().ok()?,
    )
}
