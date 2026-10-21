// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Clipboard, text only for now (spec M3 baseline). Uses `CF_UNICODETEXT`.
//!
//! The clipboard is shared with every application, so these calls open it briefly and close
//! it on every path. Any thread may call them; the clipboard is not tied to the UI thread.

use std::ptr::null_mut;
use std::thread::sleep;
use std::time::Duration;

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
/// Another process may hold the clipboard for a moment. Try a few times before giving up.
const OPEN_ATTEMPTS: u32 = 10;

/// Closes the clipboard when dropped, so every return path releases it.
struct Open;

impl Open {
    fn new() -> Result<Self, String> {
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: a null owner opens the clipboard for this thread.
            if unsafe { OpenClipboard(null_mut()) } != 0 {
                return Ok(Open);
            }
            if attempt + 1 < OPEN_ATTEMPTS {
                sleep(Duration::from_millis(10));
            }
        }
        Err("the clipboard is held by another application".into())
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: balances the successful OpenClipboard in `new`.
        unsafe { CloseClipboard() };
    }
}

/// Returns the clipboard's text, or `None` when it holds no text.
pub fn get_text() -> Result<Option<String>, String> {
    let _open = Open::new()?;
    // SAFETY: the clipboard is open on this thread.
    let handle: HANDLE = unsafe { GetClipboardData(CF_UNICODETEXT) };
    if handle.is_null() {
        return Ok(None);
    }
    // SAFETY: a text handle from GetClipboardData is a global memory block; it stays valid
    // while the clipboard is open, and it is unlocked below.
    unsafe {
        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            return Err("GlobalLock failed on clipboard text".into());
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        GlobalUnlock(handle);
        Ok(Some(text))
    }
}

/// Replaces the clipboard's contents with `text`.
pub fn set_text(text: &str) -> Result<(), String> {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len() * std::mem::size_of::<u16>();
    // SAFETY: a movable global block of `bytes` bytes, filled below before it is handed over.
    let mem = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if mem.is_null() {
        return Err("GlobalAlloc failed".into());
    }
    // SAFETY: `mem` is locked for the copy and unlocked before it is handed to the clipboard.
    unsafe {
        let dst = GlobalLock(mem) as *mut u16;
        if dst.is_null() {
            GlobalFree(mem);
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), dst, units.len());
        GlobalUnlock(mem);
    }

    let _open = match Open::new() {
        Ok(open) => open,
        Err(e) => {
            // SAFETY: the clipboard did not take the block, so we still own it.
            unsafe { GlobalFree(mem) };
            return Err(e);
        }
    };
    // SAFETY: the clipboard is open on this thread.
    unsafe { EmptyClipboard() };
    // SAFETY: on success the clipboard owns `mem`; on failure we still do and free it.
    let stored = unsafe { SetClipboardData(CF_UNICODETEXT, mem as HANDLE) };
    if stored.is_null() {
        // SAFETY: the clipboard refused the block, so it is still ours to free.
        unsafe { GlobalFree(mem) };
        return Err("SetClipboardData failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::interactive_desktop_available;

    #[test]
    fn clipboard_text_round_trips_and_the_old_contents_come_back() {
        if !interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        let before = get_text().expect("read clipboard");
        let sample = "tpt-shell clipboard test: héllo ✓ and\nnewline";
        set_text(sample).expect("write clipboard");
        assert_eq!(get_text().expect("read back").as_deref(), Some(sample));
        // Put back what was there, so the test leaves the user's clipboard alone.
        match before {
            Some(text) => set_text(&text).expect("restore clipboard"),
            None => {
                // SAFETY: opens and empties the clipboard for this thread.
                let _open = Open::new().expect("reopen clipboard");
                unsafe { EmptyClipboard() };
            }
        }
    }
}
