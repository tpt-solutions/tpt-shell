// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A hidden top-level window that hosts a WebView2 controller, plus a message pump.
//! WebView2 completes its asynchronous setup through the UI thread's message queue, so
//! waiting for it means pumping messages.

use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::Sleep;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, TranslateMessage, CW_USEDEFAULT, MSG, PM_REMOVE, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW,
};

use crate::com::wide;

const CLASS_NAME: &str = "tpt_shell_host_window";

pub struct HiddenWindow {
    hwnd: HWND,
}

impl HiddenWindow {
    /// Creates a window that is never shown. Registering the class is idempotent: if it
    /// already exists, RegisterClassExW fails and CreateWindowExW then reports the real error.
    pub fn new() -> Result<Self, String> {
        // SAFETY: a null name asks for the module handle of the current executable.
        let hinstance = unsafe { GetModuleHandleW(null()) };
        let class = wide(CLASS_NAME);
        let title = wide("tpt-shell");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: `wc` and the strings it points to live for the duration of the call.
        unsafe { RegisterClassExW(&wc) };
        // SAFETY: all pointer arguments are null or point at live NUL-terminated strings.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                800,
                600,
                null_mut(),
                null_mut(),
                hinstance,
                null(),
            )
        };
        if hwnd.is_null() {
            return Err("CreateWindowExW failed".into());
        }
        Ok(HiddenWindow { hwnd })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        // SAFETY: `hwnd` was created by us and is destroyed once, here.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Pumps this thread's messages until `done` returns true or `timeout` passes.
/// Returns whether `done` became true.
pub fn pump_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // SAFETY: `msg` is a valid out-pointer; a null window filter takes all messages.
        while unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: `msg` was just filled in by PeekMessageW.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if done() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        // SAFETY: Sleep has no preconditions.
        unsafe { Sleep(1) };
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
