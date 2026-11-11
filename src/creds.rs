use crate::local::wide;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

const TARGET: &str = "GeoStoreUpdater/portal.geostore.cz";

pub fn save(user: &str, pass: &str) -> anyhow::Result<()> {
    let target = wide(TARGET);
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

pub fn load() -> Option<(String, String)> {
    let target = wide(TARGET);
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

pub fn exists() -> bool {
    load().is_some()
}

pub fn delete() {
    let target = wide(TARGET);
    unsafe {
        let _ = CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0);
    }
}
