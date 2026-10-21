// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Per-user registry values under `HKEY_CURRENT_USER` (spec §6.1). Deep links and the startup
//! entry are both per user, so they need no administrator rights and touch no other account.
//! Only string values are used.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

use crate::com::wide;

/// Sets a string value under `subkey`, creating the key if needed. `name` of `None` sets the
/// key's default value.
pub fn set_string(subkey: &str, name: Option<&str>, value: &str) -> Result<(), String> {
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

/// The string value `name` under `subkey`, or `None` when the key or value is missing.
pub fn get_string(subkey: &str, name: Option<&str>) -> Option<String> {
    let key_path = wide(subkey);
    let name_w = name.map(wide);
    let name_ptr = name_w.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());
    let mut size: u32 = 0;
    // SAFETY: a null buffer asks only for the size of the value.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            name_ptr,
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
            key_path.as_ptr(),
            name_ptr,
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

/// Removes the named value under `subkey`. A missing value is not an error.
pub fn delete_value(subkey: &str, name: &str) -> Result<(), String> {
    let key_path = wide(subkey);
    let name_w = wide(name);
    let mut key: HKEY = null_mut();
    // SAFETY: the subkey is NUL-terminated for the call; `key` is closed below.
    let status = unsafe {
        windows_sys::Win32::System::Registry::RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            KEY_ALL_ACCESS,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return Ok(());
    }
    // SAFETY: `key` is open and `name_w` is NUL-terminated for the call.
    let status = unsafe { RegDeleteValueW(key, name_w.as_ptr()) };
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    if status == ERROR_SUCCESS || status == 2 {
        Ok(())
    } else {
        Err(format!("removing {name} failed: error {status}"))
    }
}

/// Removes the whole key `subkey` and everything under it. A missing key is not an error.
pub fn delete_tree(subkey: &str) -> Result<(), String> {
    let key_path = wide(subkey);
    // SAFETY: the subkey is NUL-terminated for the call.
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key_path.as_ptr()) };
    if status == ERROR_SUCCESS || status == 2 {
        Ok(())
    } else {
        Err(format!("removing {subkey} failed: error {status}"))
    }
}
