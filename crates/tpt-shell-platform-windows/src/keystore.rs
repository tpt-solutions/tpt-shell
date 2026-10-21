// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Keystore on Windows (spec §6.1, the `Platform` trait). Secrets are encrypted with DPAPI for
//! the current user, so another account on the machine cannot read them, and each secret is
//! stored as its own file in the keystore folder. Nothing is ever written in plain text.

use std::fs;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::com::wide;

/// Encrypts `plain` for the current user. `description` is stored with the ciphertext.
pub fn protect(plain: &[u8], description: &str) -> Result<Vec<u8>, String> {
    let input = blob_of(plain)?;
    let label = wide(description);
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // SAFETY: `input` points at `plain`, which lives for the call. The label is NUL-terminated.
    // The UI flag keeps the call from prompting, so it never waits on a user.
    let ok = unsafe {
        CryptProtectData(
            &input,
            label.as_ptr(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!("CryptProtectData failed: error {}", last_error()));
    }
    Ok(take_blob(output))
}

/// Decrypts a blob made by [`protect`] for the current user.
pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>, String> {
    let input = blob_of(sealed)?;
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: null_mut(),
    };
    // SAFETY: as in `protect`. The description out-pointer is allowed to be null.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            null_mut(),
            null(),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!("CryptUnprotectData failed: error {}", last_error()));
    }
    Ok(take_blob(output))
}

fn blob_of(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, String> {
    let cb = u32::try_from(bytes.len()).map_err(|_| "the secret is too large".to_string())?;
    Ok(CRYPT_INTEGER_BLOB {
        cbData: cb,
        pbData: bytes.as_ptr() as *mut u8,
    })
}

/// Copies a blob the system allocated, then frees it.
fn take_blob(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if blob.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: the system allocated `cbData` bytes at `pbData` with LocalAlloc, and we free it
    // exactly once, after copying.
    let bytes = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
    unsafe { LocalFree(blob.pbData as HLOCAL) };
    bytes
}

fn last_error() -> u32 {
    // SAFETY: GetLastError reads the calling thread's error code and has no preconditions.
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}

/// Secrets kept in one folder, one encrypted file per name.
pub struct Keystore {
    dir: PathBuf,
}

impl Keystore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Keystore { dir: dir.into() }
    }

    /// Stores `secret` under `name`, replacing any earlier one.
    pub fn set(&self, name: &str, secret: &[u8]) -> Result<(), String> {
        let path = self.path_for(name)?;
        fs::create_dir_all(&self.dir).map_err(|e| format!("cannot create the keystore: {e}"))?;
        let sealed = protect(secret, &format!("tpt-shell:{name}"))?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, sealed).map_err(|e| format!("cannot write the secret: {e}"))?;
        fs::rename(&tmp, &path).map_err(|e| format!("cannot save the secret: {e}"))
    }

    /// The secret stored under `name`, or `None` when there is none.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.path_for(name)?;
        match fs::read(&path) {
            Ok(sealed) => unprotect(&sealed).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read the secret: {e}")),
        }
    }

    /// Removes the secret under `name`. Returns whether there was one.
    pub fn delete(&self, name: &str) -> Result<bool, String> {
        let path = self.path_for(name)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!("cannot delete the secret: {e}")),
        }
    }

    /// Names are letters, digits, `-`, `.` and `_`, so a name cannot leave the folder.
    fn path_for(&self, name: &str) -> Result<PathBuf, String> {
        let ok = !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b));
        if !ok {
            return Err(format!("{name:?} is not a valid keystore name"));
        }
        Ok(self.dir.join(format!("{name}.bin")))
    }
}

/// The folder a keystore uses for `app_id`: `%LOCALAPPDATA%\tpt-shell\<app>\keystore`.
pub fn default_keystore_dir(local_app_data: &Path, app_id: &str) -> PathBuf {
    local_app_data
        .join("tpt-shell")
        .join(app_id)
        .join("keystore")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_round_trips_and_is_not_stored_in_plain_text() {
        let sealed = protect(b"hunter2", "test").expect("protect");
        assert!(
            !sealed.windows(7).any(|w| w == b"hunter2"),
            "plain text in the blob"
        );
        assert_eq!(unprotect(&sealed).expect("unprotect"), b"hunter2");
    }

    #[test]
    fn a_tampered_blob_is_refused() {
        let mut sealed = protect(b"secret", "test").expect("protect");
        let last = sealed.len() - 1;
        sealed[last] ^= 0x55;
        assert!(unprotect(&sealed).is_err());
    }

    #[test]
    fn the_keystore_sets_gets_and_deletes_by_name() {
        let dir = std::env::temp_dir().join(format!("tpt-keystore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Keystore::new(&dir);
        assert_eq!(store.get("audit-key").expect("get"), None);
        store.set("audit-key", b"key-bytes").expect("set");
        store.set("audit-key", b"new-bytes").expect("replace");
        assert_eq!(
            store.get("audit-key").expect("get"),
            Some(b"new-bytes".to_vec())
        );
        assert!(store.delete("audit-key").expect("delete"));
        assert!(!store.delete("audit-key").expect("delete again"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_that_could_leave_the_folder_are_refused() {
        let store = Keystore::new(std::env::temp_dir());
        for bad in ["", ".", "..", "a/b", "a\\b", "x:y"] {
            assert!(store.set(bad, b"x").is_err(), "{bad:?} was accepted");
        }
    }
}
