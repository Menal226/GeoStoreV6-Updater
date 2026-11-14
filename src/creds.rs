use crate::local::wide;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

const TARGET: &str = "GeoStoreUpdater/portal.geostore.cz";
const LICMAN_TARGET: &str = "GeoStoreUpdater/licman";

pub fn save(user: &str, pass: &str) -> anyhow::Result<()> {
    save_to(TARGET, user, pass)
}

pub fn load() -> Option<(String, String)> {
    load_from(TARGET)
}

pub fn exists() -> bool {
    load().is_some()
}

pub fn delete() {
    delete_target(TARGET);
}

/// A separate account for the licence server, used instead of the portal one when set.
pub fn save_licman(user: &str, pass: &str) -> anyhow::Result<()> {
    save_to(LICMAN_TARGET, user, pass)
}

pub fn load_licman() -> Option<(String, String)> {
    load_from(LICMAN_TARGET)
}

pub fn delete_licman() {
    delete_target(LICMAN_TARGET);
}

fn save_to(target: &str, user: &str, pass: &str) -> anyhow::Result<()> {
    let target = wide(target);
    let mut username = wide(user);
    let blob: Vec<u8> = pass.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr() as *mut u16),
        UserName: PWSTR(username.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0)? };
    Ok(())
}

fn load_from(target: &str) -> Option<(String, String)> {
    let target = wide(target);
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0, &mut ptr).ok()?;
        if ptr.is_null() {
            return None;
        }
        let cred = &*ptr;
        let user = cred.UserName.to_string().unwrap_or_default();
        let len = cred.CredentialBlobSize as usize / 2;
        let words = std::slice::from_raw_parts(cred.CredentialBlob as *const u16, len);
        let pass = String::from_utf16_lossy(words);
        CredFree(ptr as *const std::ffi::c_void);
        if user.is_empty() {
            None
        } else {
            Some((user, pass))
        }
    }
}

fn delete_target(target: &str) {
    let target = wide(target);
    unsafe {
        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0);
    }
}
