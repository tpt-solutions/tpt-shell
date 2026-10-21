// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Windows low-level keyboard hook for kiosk key chords (spec §10.1, best effort). The hook runs
//! on its own thread with a message loop, and blocks the chords in [`crate::chords`] before any
//! app sees them. It is user-space and can be bypassed by the OS (for example Ctrl+Alt+Del); the
//! docs say so. Unsafe code is limited to this module.

use std::io;
use std::sync::mpsc;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
};

use crate::chords;

/// The low-level flag for "Alt is down" in `KBDLLHOOKSTRUCT::flags`.
const LLKHF_ALTDOWN: u32 = 0x20;
/// `VK_CONTROL`.
const VK_CONTROL: i32 = 0x11;

/// Keeps the hook installed. Dropping it removes the hook and stops its thread.
pub struct KeyboardHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

/// Installs the hook. Fails when Windows refuses it, with the OS error.
pub fn install() -> io::Result<KeyboardHook> {
    let (tx, rx) = mpsc::channel::<Result<u32, u32>>();
    let thread = std::thread::Builder::new()
        .name("tpt-kiosk-keys".into())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId has no preconditions.
            let thread_id = unsafe { GetCurrentThreadId() };
            // SAFETY: a null module name returns the handle of the running executable. The
            // procedure is `hook_proc`, which lives for the whole program.
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    GetModuleHandleW(std::ptr::null()),
                    0,
                )
            };
            if hook.is_null() {
                // SAFETY: GetLastError has no preconditions.
                let _ = tx.send(Err(unsafe { GetLastError() }));
                return;
            }
            let _ = tx.send(Ok(thread_id));
            let mut msg: MSG = unsafe_zeroed_msg();
            // SAFETY: `msg` is a valid out-pointer. The loop ends at WM_QUIT (0) or an error (-1).
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
            // SAFETY: `hook` came from SetWindowsHookExW on this thread and is removed once.
            unsafe { UnhookWindowsHookEx(hook) };
        })?;
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(KeyboardHook {
            thread_id,
            thread: Some(thread),
        }),
        Ok(Err(code)) => {
            let _ = thread.join();
            Err(io::Error::from_raw_os_error(code as i32))
        }
        Err(_) => {
            let _ = thread.join();
            Err(io::Error::other("the keyboard hook thread stopped early"))
        }
    }
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to a thread id is safe even if that thread has ended.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn unsafe_zeroed_msg() -> MSG {
    // SAFETY: MSG is a plain C struct of integers and pointers, and all-zero is a valid value.
    unsafe { std::mem::zeroed() }
}

/// The hook procedure. Blocks a key-down that is in the chord list by returning non-zero.
///
/// SAFETY (contract with Windows): when `code` is non-negative, `lparam` points to a valid
/// `KBDLLHOOKSTRUCT` for the duration of the call.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && (wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize) {
        // SAFETY: Windows guarantees the pointer, as stated above.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let alt = info.flags & LLKHF_ALTDOWN != 0;
        // SAFETY: GetAsyncKeyState has no preconditions.
        let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL) } as u16 & 0x8000 != 0;
        if chords::is_blocked(info.vkCode, alt, ctrl) {
            return 1;
        }
    }
    // SAFETY: passes the same arguments on, as the hook chain requires.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_installs_and_removes_cleanly_and_can_be_installed_again() {
        let first = install().expect("install the keyboard hook");
        drop(first);
        let second = install().expect("install again after removal");
        drop(second);
    }
}
