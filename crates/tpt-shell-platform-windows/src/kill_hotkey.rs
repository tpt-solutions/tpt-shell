// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Global kill-switch hotkey (spec §9). A press sets the agent session's kill switch: the flag
//! that [`tpt_shell_agent::Session::kill_switch`] returns. From then on every agent call is
//! refused.
//!
//! The key is registered with `RegisterHotKey` on a thread of its own, which runs a message
//! loop. That keeps the hotkey working while the UI thread is busy, and it needs no window of
//! the app.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{GetLastError, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_HOTKEY, WM_QUIT,
};

/// Identifies the hotkey on its thread. One hotkey per thread.
const HOTKEY_ID: i32 = 1;
/// `ERROR_HOTKEY_ALREADY_REGISTERED`: another app, or an earlier registration, holds the key.
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

/// Ctrl+Alt+Shift, without key repeat. Pair it with [`DEFAULT_KEY`] for the default hotkey.
pub const DEFAULT_MODIFIERS: u32 = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
/// The virtual-key code of K. With [`DEFAULT_MODIFIERS`]: Ctrl+Alt+Shift+K.
pub const DEFAULT_KEY: u32 = 0x4B;

/// A registered kill-switch hotkey. Dropping it unregisters the key and stops its thread.
pub struct KillHotkey {
    thread: u32,
    handle: Option<JoinHandle<()>>,
}

impl KillHotkey {
    /// Registers `modifiers` plus the virtual key `key` as the kill switch for `kill`. A press
    /// stores `true` in `kill`, which is the flag `Session::kill_switch()` returns.
    ///
    /// Support: Partial. Windows gives a key combination to one app only. When another app
    /// already holds it, this returns an error and registers nothing; pick another combination.
    /// The key is not seen while the session is locked or on the secure desktop. Only Windows
    /// has this hotkey.
    pub fn register(kill: Arc<AtomicBool>, modifiers: u32, key: u32) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("tpt-kill-hotkey".into())
            .spawn(move || run(kill, modifiers, key, tx))
            .map_err(|e| format!("cannot start the kill-switch thread: {e}"))?;
        match rx.recv() {
            Ok(Ok(thread)) => Ok(KillHotkey {
                thread,
                handle: Some(handle),
            }),
            Ok(Err(reason)) => {
                let _ = handle.join();
                Err(reason)
            }
            Err(_) => {
                let _ = handle.join();
                Err("the kill-switch thread stopped before it registered the key".into())
            }
        }
    }

    /// Posts a hotkey message to the thread, as a press would. Tests only.
    #[cfg(test)]
    fn press_for_test(&self) -> bool {
        // SAFETY: posting to a thread id is thread-safe; the id is that of our live thread.
        unsafe { PostThreadMessageW(self.thread, WM_HOTKEY, HOTKEY_ID as WPARAM, 0) != 0 }
    }
}

impl Drop for KillHotkey {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to our thread ends its loop; the thread unregisters the key.
        unsafe { PostThreadMessageW(self.thread, WM_QUIT, 0, 0) };
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// The hotkey thread: registers the key, reports the outcome, then pumps messages until quit.
fn run(kill: Arc<AtomicBool>, modifiers: u32, key: u32, tx: mpsc::Sender<Result<u32, String>>) {
    // SAFETY: GetCurrentThreadId has no preconditions.
    let thread = unsafe { GetCurrentThreadId() };
    // SAFETY: a PM_NOREMOVE peek only creates this thread's message queue, so that
    // PostThreadMessage can reach it before the first GetMessage.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
    }
    // SAFETY: a null window makes the hotkey post to this thread's queue.
    let ok = unsafe { RegisterHotKey(null_mut(), HOTKEY_ID, modifiers, key) };
    if ok == 0 {
        // SAFETY: GetLastError reads this thread's last error code.
        let code = unsafe { GetLastError() };
        let reason = if code == ERROR_HOTKEY_ALREADY_REGISTERED {
            "the kill-switch key is already taken by another app; choose another key".to_string()
        } else {
            format!("RegisterHotKey failed: error {code}")
        };
        let _ = tx.send(Err(reason));
        return;
    }
    if tx.send(Ok(thread)).is_err() {
        // SAFETY: balances the successful registration above.
        unsafe { UnregisterHotKey(null_mut(), HOTKEY_ID) };
        return;
    }
    loop {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `msg` is a valid out-pointer; a null window takes this thread's messages.
        let got = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if got <= 0 {
            break;
        }
        if msg.message == WM_HOTKEY && msg.wParam == HOTKEY_ID as WPARAM {
            kill.store(true, Ordering::SeqCst);
        }
    }
    // SAFETY: balances the successful registration above.
    unsafe { UnregisterHotKey(null_mut(), HOTKEY_ID) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::interactive_desktop_available;
    use std::time::{Duration, Instant};

    #[test]
    fn a_press_sets_the_kill_switch_and_a_taken_key_is_refused() {
        if !interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        // Ctrl+Alt+Shift+F24 is almost never bound by another app.
        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        let kill = Arc::new(AtomicBool::new(false));
        let hotkey = KillHotkey::register(kill.clone(), mods, 0x87).expect("register");
        let second = KillHotkey::register(Arc::new(AtomicBool::new(false)), mods, 0x87);
        assert!(second.is_err(), "the same key registered twice");

        assert!(hotkey.press_for_test());
        let start = Instant::now();
        while !kill.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            kill.load(Ordering::SeqCst),
            "the press did not set the flag"
        );
        drop(hotkey);
    }
}
