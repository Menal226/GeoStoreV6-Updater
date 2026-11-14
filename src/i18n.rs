use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    #[serde(rename = "cs")]
    Cs,
    #[serde(rename = "en")]
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Cs, Lang::En];

    /// Each language is named in itself, so people can find theirs whichever one is active.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Cs => "Čeština",
            Lang::En => "English",
        }
    }

    /// Czech and Slovak Windows get Czech, everything else English.
    pub fn system() -> Lang {
        let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        match id & 0x3ff {
            0x05 | 0x1b => Lang::Cs,
            _ => Lang::En,
        }
    }

    fn table(self) -> &'static HashMap<String, String> {
        static CS: OnceLock<HashMap<String, String>> = OnceLock::new();
        static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
        let (cell, source) = match self {
            Lang::Cs => (&CS, include_str!("../lang/cs.json")),
            Lang::En => (&EN, include_str!("../lang/en.json")),
        };
        cell.get_or_init(|| {
            let mut table: HashMap<String, String> = serde_json::from_str(source).unwrap_or_default();
            if self == Lang::Cs {
                table.values_mut().for_each(|v| *v = tie_short_words(v));
            }
            table
        })
    }
}

/// Czech typography: a one-letter word (k, s, v, z, o, u, a, i) must not end a line, so the space after it becomes non-breaking.
fn tie_short_words(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        let single = i > 0
            && "ksvzouaiKSVZOUAI".contains(chars[i - 1])
            && (i < 2 || !chars[i - 2].is_alphanumeric());
        out.push(if c == ' ' && single { '\u{A0}' } else { c });
    }
    out
}

/// Text with non-breaking spaces turned back into plain ones, for comparing log lines with templates.
pub fn plain(text: &str) -> String {
    text.replace('\u{A0}', " ")
}

static CURRENT: AtomicU8 = AtomicU8::new(Lang::Cs as u8);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == Lang::En as u8 {
        Lang::En
    } else {
        Lang::Cs
    }
}

pub fn get_in(lang: Lang, key: &'static str) -> &'static str {
    lang.table()
        .get(key)
        .or_else(|| Lang::Cs.table().get(key))
        .map_or(key, String::as_str)
}

pub fn get(key: &'static str) -> &'static str {
    get_in(current(), key)
}

/// Picks `key.one`, `key.few` or `key.other` for `n`. English has no "few" form, so it falls back to "other".
pub fn plural(key: &'static str, n: u64) -> &'static str {
    let form = match (current(), n) {
        (_, 1) => "one",
        (Lang::Cs, 2..=4) => "few",
        _ => "other",
    };
    let table = current().table();
    table
        .get(&format!("{key}.{form}"))
        .or_else(|| table.get(&format!("{key}.other")))
        .map_or(key, String::as_str)
}

/// Replaces `{name}` placeholders in a translated template.
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// Log text written in another language, in the current one, when it is a fixed string without placeholders.
pub fn translate_fixed(text: &str) -> Option<&'static str> {
    let cur = current();
    Lang::ALL.iter().filter(|&&l| l as u8 != cur as u8).find_map(|&lang| {
        lang.table()
            .iter()
            .find(|(_, v)| !v.contains('{') && plain(v).trim_end_matches('.') == text)
            .and_then(|(k, _)| cur.table().get(k))
            .map(|v| v.trim_end_matches('.'))
    })
}

/// Whether `text` starts with the fixed part of `key` in any language. For text read back from the log, which may have been written in another language.
pub fn starts_with_any(text: &str, key: &'static str) -> bool {
    Lang::ALL.iter().any(|&lang| {
        let template = plain(get_in(lang, key));
        let prefix = template.split('{').next().unwrap_or_default();
        !prefix.is_empty() && text.starts_with(prefix)
    })
}

/// `t!("key")` gives the text in the current language; `t!("key", name = value, ...)` also fills `{name}` placeholders.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::i18n::get($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::get($key), &[$((stringify!($name), ($value).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn placeholders(text: &str) -> BTreeSet<String> {
        regex::Regex::new(r"\{([a-z_0-9]+)\}").unwrap().captures_iter(text).map(|c| c[1].to_string()).collect()
    }

    fn sources(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(std::fs::read_to_string(&path).unwrap());
            }
        }
    }

    #[test]
    fn languages_have_the_same_keys_and_placeholders() {
        let cs = Lang::Cs.table();
        let en = Lang::En.table();
        assert!(!cs.is_empty() && !en.is_empty(), "a language file failed to parse");
        let cs_keys: BTreeSet<_> = cs.keys().filter(|k| !k.ends_with(".few")).collect();
        let en_keys: BTreeSet<_> = en.keys().collect();
        assert_eq!(cs_keys.difference(&en_keys).collect::<Vec<_>>(), Vec::<&&String>::new(), "missing in en.json");
        assert_eq!(en_keys.difference(&cs_keys).collect::<Vec<_>>(), Vec::<&&String>::new(), "missing in cs.json");
        for (key, text) in cs {
            let other = en.get(key).or_else(|| en.get(&key.replace(".few", ".other"))).unwrap();
            assert_eq!(placeholders(text), placeholders(other), "placeholders differ for {key}");
        }
    }

    #[test]
    fn every_key_used_in_the_code_exists() {
        let cs = Lang::Cs.table();
        let sections: BTreeSet<&str> = cs.keys().filter_map(|k| k.split('.').next()).collect();
        let mut files = Vec::new();
        sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let literal = regex::Regex::new(r#""([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+)""#).unwrap();
        let mut missing = BTreeSet::new();
        for file in &files {
            for c in literal.captures_iter(file) {
                let key = &c[1];
                let section = key.split('.').next().unwrap();
                if ["json", "log", "cz", "exe", "bat", "txt", "lic", "zip", "msi", "html", "aspx", "lock"].contains(&key.rsplit('.').next().unwrap()) {
                    continue;
                }
                if sections.contains(section) && !cs.contains_key(key) && !cs.contains_key(&format!("{key}.other")) {
                    missing.insert(key.to_string());
                }
            }
        }
        assert!(missing.is_empty(), "keys used in the code but missing in cs.json: {missing:?}");
    }
}
