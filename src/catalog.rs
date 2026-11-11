pub const EDITOR: &str = "EDITOR";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Partial,
    Complete,
    Single,
}

#[derive(Clone, Debug)]
pub struct ModuleInfo {
    pub name: String,
    pub description: String,
    pub initials: String,
    pub key_files: Vec<&'static str>,
    pub version_file: Option<&'static str>,
    pub order: u32,
}

pub fn module_info(key: &str) -> ModuleInfo {
    let (name, description, initials, key_files, version_file, order): (
        String,
        &str,
        String,
        Vec<&'static str>,
        Option<&'static str>,
        u32,
    ) = match key {
        EDITOR => (
            "GeoStore V6".into(),
            "Základní grafický editor",
            "GS".into(),
            vec!["GeoStore.exe"],
            Some("GeoStore.exe"),
            0,
        ),
        "DTMCR" => (
            "V6-DTMČR".into(),
            "Digitální technická mapa ČR",
            "DTM".into(),
            vec!["V6_dtmcr.dll"],
            Some("V6_dtmcr.dll"),
            1,
        ),
        "IG" => (
            "V6-IG".into(),
            "Inženýrská geodézie",
            "IG".into(),
            vec!["techline.dll"],
            Some("techline.dll"),
            2,
        ),
        "3D" => (
            "V6-3D".into(),
            "3D a laserové skenování",
            "3D".into(),
            vec!["V6_3D.dll"],
            Some("V6_3D.dll"),
            3,
        ),
        "DWG" => (
            "Rozšíření DWG-DGN".into(),
            "Práce s výkresy DWG a DGN",
            "DWG".into(),
            vec!["GSv6Dwg.dll", "GSDwgCore.dll"],
            Some("GSv6Dwg.dll"),
            4,
        ),
        other => (
            format!("V6-{other}"),
            "Aplikační řešení",
            other.chars().take(3).collect(),
            vec![],
            None,
            10,
        ),
    };
    ModuleInfo {
        name,
        description: description.to_string(),
        initials,
        key_files,
        version_file,
        order,
    }
}

pub fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'č' => 'c',
            'ď' => 'd',
            'é' | 'ě' => 'e',
            'í' => 'i',
            'ň' => 'n',
            'ó' => 'o',
            'ř' => 'r',
            'š' => 's',
            'ť' => 't',
            'ú' | 'ů' => 'u',
            'ý' => 'y',
            'ž' => 'z',
            c => c,
        })
        .collect()
}

fn module_key(s: &str) -> String {
    let k = s.trim_matches('_').to_uppercase();
    match k.as_str() {
        "3DLAS" | "3D_LAS" | "LAS" => "3D".to_string(),
        _ => k,
    }
}

pub fn classify(title: &str) -> Option<(String, Variant)> {
    let n = normalize(title.trim());
    let stem = n.strip_suffix(".zip")?;
    if stem.starts_with("zakladni_graficky_editor") {
        return stem
            .contains("x64")
            .then(|| (EDITOR.to_string(), Variant::Single));
    }
    if let Some(rest) = stem.strip_prefix("castecna_instalace_aplikace_") {
        return Some((module_key(rest), Variant::Partial));
    }
    if let Some(rest) = stem.strip_prefix("kompletni_instalace_aplikace_") {
        let rest = rest.split("_hw_klic").next().unwrap_or(rest);
        let rest = rest.trim_end_matches("_x64");
        return Some((module_key(rest), Variant::Complete));
    }
    if let Some(rest) = stem.strip_prefix("instalace_rozsireni_") {
        if rest.contains("dwg") {
            return Some(("DWG".to_string(), Variant::Single));
        }
        let rest = rest.trim_start_matches("gsv6_").trim_end_matches("_x64");
        return Some((module_key(rest), Variant::Single));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_portal_files() {
        let cases = [
            ("castecna_instalace_aplikace_IG.zip", Some(("IG", Variant::Partial))),
            ("kompletní_instalace_aplikace_IG_HW_klic_x64.zip", Some(("IG", Variant::Complete))),
            ("castecna_instalace_aplikace_DTMCR.zip", Some(("DTMCR", Variant::Partial))),
            ("kompletni_instalace_aplikace_DTMCR_HW_klic_x64.zip", Some(("DTMCR", Variant::Complete))),
            ("castecna_instalace_aplikace_3D.zip", Some(("3D", Variant::Partial))),
            ("kompletni_instalace_aplikace_3DLAS_HW_klic_x64.zip", Some(("3D", Variant::Complete))),
            ("instalace_rozšíření_GSV6_DWG_DGN_x64.zip", Some(("DWG", Variant::Single))),
            ("zakladni_graficky_editor_GSV6_x64.zip", Some((EDITOR, Variant::Single))),
            ("viewer_V6_HWKEY_x64.zip", None),
            ("Sledování změn řešení.xlsx", None),
            ("aplikace_pro_zjisteni_ID_klice.zip", None),
        ];
        for (title, expected) in cases {
            let got = classify(title);
            let expected = expected.map(|(k, v)| (k.to_string(), v));
            assert_eq!(got, expected, "{title}");
        }
    }
}
