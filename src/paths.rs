use std::path::PathBuf;

pub const GEOSTORE_DIR: &str = r"C:\Program Files\GEOVAP\GeoStoreV6";
pub const KONFIG_DIR: &str = r"C:\V6-KONFIG";
pub const BACKUP_ROOT: &str = r"C:\V6-KONFIG_zalohy";

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let dir = base.join("GeoStore Updater");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn sub(name: &str) -> PathBuf {
    let dir = data_dir().join(name);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn downloads_dir() -> PathBuf {
    sub("downloads")
}

pub fn extract_dir() -> PathBuf {
    sub("extract")
}

pub fn logs_dir() -> PathBuf {
    sub("logs")
}

pub fn davka_log() -> PathBuf {
    PathBuf::from(GEOSTORE_DIR).join("davka.log")
}
