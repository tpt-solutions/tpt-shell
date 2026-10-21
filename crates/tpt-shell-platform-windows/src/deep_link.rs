// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Deep links on Windows (spec §6.1, the `Platform` trait). A URL scheme such as `myapp://` is
//! registered for the current user under `HKCU\Software\Classes`, and the link starts the app
//! with the URL as its argument. A running app receives it through single instance.
//!
//! Registration is per user, so it needs no administrator rights and touches no other account.

use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

use crate::com::wide;

/// Schemes a page or another app already relies on. Registering one would take it over.
const RESERVED: &[&str] = &[
    "http",
    "https",
    "file",
    "javascript",
    "data",
    "about",
    "mailto",
    "ms",
];

/// Registers `scheme` for the current user, so that links to it start `exe` with the link as
/// the argument.
pub fn register(scheme: &str, exe: &Path, description: &str) -> Result<(), String> {
    check_scheme(scheme)?;
    let root = format!(r"Software\Classes\{scheme}");
    let command = format!("\"{}\" \"%1\"", exe.display());
    set_value(&root, None, &format!("URL:{description}"))?;
    set_value(&root, Some("URL Protocol"), "")?;
    set_value(&format!(r"{root}\shell\open\command"), None, &command)?;
    Ok(())
}

/// Removes a registration made by [`register`]. Removing one that is not there is not an error.
pub fn unregister(scheme: &str) -> Result<(), String> {
    check_scheme(scheme)?;
    let root = wide(&format!(r"Software\Classes\{scheme}"));
    // SAFETY: the root is HKEY_CURRENT_USER, and the subkey is NUL-terminated for the call.
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, root.as_ptr()) };
    if status == ERROR_SUCCESS || status == 2 {
        Ok(())
    } else {
        Err(format!("removing the scheme failed: error {status}"))
    }
}

/// The command a scheme starts, or `None` when it is not registered.
pub fn registered_command(scheme: &str) -> Option<String> {
    let key = wide(&format!(r"Software\Classes\{scheme}\shell\open\command"));
    let mut size: u32 = 0;
    // SAFETY: a null buffer asks only for the size of the value.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            null_mut(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size < 2 {
        return None;
    }
    let mut buffer = vec![0u16; size as usize / 2];
    // SAFETY: `buffer` holds `size` bytes, as the size query said.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            null_mut(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let end = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

/// Schemes are a letter followed by letters, digits, `+`, `-` or `.`, and are not reserved.
fn check_scheme(scheme: &str) -> Result<(), String> {
    let mut chars = scheme.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !starts_well || !rest_ok {
        return Err(format!("{scheme:?} is not a valid URL scheme"));
    }
    if RESERVED.contains(&scheme.to_ascii_lowercase().as_str()) {
        return Err(format!("{scheme:?} is reserved and cannot be registered"));
    }
    Ok(())
}

/// Creates `subkey` under HKEY_CURRENT_USER and sets its default value or a named value.
fn set_value(subkey: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let key_path = wide(subkey);
    let mut key: HKEY = null_mut();
    // SAFETY: the subkey is NUL-terminated for the call. `key` receives the opened handle, which
    // is closed below.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            null_mut(),
            &mut key,
            null_mut(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("creating {subkey} failed: error {status}"));
    }
    let name_w = name.map(wide);
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `data` is NUL-terminated UTF-16 and lives for the call; its byte length is given.
    let status = unsafe {
        RegSetValueExW(
            key,
            name_w.as_ref().map_or(null_mut(), |n| n.as_ptr()),
            0,
            REG_SZ,
            data.as_ptr() as *const u8,
            (data.len() * 2) as u32,
        )
    };
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "setting a value under {subkey} failed: error {status}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_is_registered_to_the_executable_and_removed_again() {
        let scheme = format!("tpttest{}", std::process::id());
        let exe = Path::new(r"C:\Program Files\tpt-app\app.exe");
        register(&scheme, exe, "Test app").expect("register");
        assert_eq!(
            registered_command(&scheme).as_deref(),
            Some(r#""C:\Program Files\tpt-app\app.exe" "%1""#)
        );
        unregister(&scheme).expect("unregister");
        assert_eq!(registered_command(&scheme), None);
        unregister(&scheme).expect("unregistering twice is fine");
    }

    #[test]
    fn reserved_and_malformed_schemes_are_refused() {
        let exe = Path::new(r"C:\app.exe");
        for bad in ["https", "HTTP", "file", "1app", "my app", "a/b", ""] {
            assert!(register(bad, exe, "x").is_err(), "{bad:?} was accepted");
        }
    }
}
