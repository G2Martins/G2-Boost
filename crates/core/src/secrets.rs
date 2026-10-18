//! AI provider API keys in the Windows Credential Manager (DPAPI-protected per user),
//! never in settings.json or browser storage. Target names: `G2 Boost/ai/<provider>`.

use std::ffi::c_void;

use windows::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};
use windows::core::{PCWSTR, PWSTR};

use crate::registry::wide;

pub const PROVIDERS: &[&str] = &["anthropic", "openai", "gemini", "nvidia"];
const MAX_KEY_LEN: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("unknown_provider")]
    UnknownProvider,
    #[error("invalid_key")]
    InvalidKey,
    #[error("credential manager: {0}")]
    Win32(String),
}

fn target(provider: &str) -> Result<Vec<u16>, SecretError> {
    if !PROVIDERS.contains(&provider) {
        return Err(SecretError::UnknownProvider);
    }
    Ok(wide(&format!("G2 Boost/ai/{provider}")))
}

pub fn set(provider: &str, key: &str) -> Result<(), SecretError> {
    let key = key.trim();
    if key.is_empty() || key.len() > MAX_KEY_LEN || !key.chars().all(|c| c.is_ascii_graphic()) {
        return Err(SecretError::InvalidKey);
    }
    let mut t = target(provider)?;
    let mut user = wide("G2 Boost");
    let mut blob = key.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(t.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: all pointers reference buffers alive for the duration of the call.
    unsafe { CredWriteW(&cred, 0) }.map_err(|e| SecretError::Win32(e.message()))
}

pub fn get(provider: &str) -> Result<Option<String>, SecretError> {
    let t = target(provider)?;
    let mut p: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success `p` points to a CredFree-owned CREDENTIALW.
    match unsafe { CredReadW(PCWSTR(t.as_ptr()), CRED_TYPE_GENERIC, None, &mut p) } {
        Ok(()) => {
            // SAFETY: blob is `CredentialBlobSize` bytes inside the allocation; freed once below.
            let key = unsafe {
                let c = &*p;
                let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
                let s = String::from_utf8_lossy(bytes).into_owned();
                CredFree(p as *const c_void);
                s
            };
            Ok(Some(key))
        }
        Err(e) if e.code().0 as u32 == 0x8007_0490 => Ok(None), // ERROR_NOT_FOUND
        Err(e) => Err(SecretError::Win32(e.message())),
    }
}

pub fn delete(provider: &str) -> Result<(), SecretError> {
    let t = target(provider)?;
    // SAFETY: valid target string.
    match unsafe { CredDeleteW(PCWSTR(t.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code().0 as u32 == 0x8007_0490 => Ok(()),
        Err(e) => Err(SecretError::Win32(e.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes, reads and deletes a throwaway credential. Ignored by default: it touches the
    /// user's Credential Manager (run once by hand).
    #[test]
    #[ignore]
    fn roundtrip() {
        set("gemini", "test-key-123").unwrap();
        assert_eq!(get("gemini").unwrap().as_deref(), Some("test-key-123"));
        delete("gemini").unwrap();
        assert_eq!(get("gemini").unwrap(), None);
    }

    #[test]
    fn validation() {
        assert!(matches!(set("evil", "k"), Err(SecretError::UnknownProvider)));
        assert!(matches!(set("openai", ""), Err(SecretError::InvalidKey)));
        assert!(matches!(set("openai", "has space"), Err(SecretError::InvalidKey)));
        assert!(matches!(get("../x"), Err(SecretError::UnknownProvider)));
    }
}
