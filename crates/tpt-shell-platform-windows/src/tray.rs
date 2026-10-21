// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Notification-area (tray) icon with a context menu (spec M1, the `Platform` trait).
//!
//! A left click reports id [`ACTIVATE`]. A right click opens the menu, and the id of the item
//! picked is reported. Menu ids must be non-zero, because zero means "nothing picked".
//! The icon is the default application icon for now.

use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, GetWindowLongPtrW, LoadIconW, PostMessageW, RegisterClassExW,
    SetForegroundWindow, SetWindowLongPtrW, TrackPopupMenu, GWLP_USERDATA, HWND_MESSAGE,
    IDI_APPLICATION, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSEXW,
};

use crate::com::wide;

/// The id reported for a left click on the icon.
pub const ACTIVATE: u32 = 0;

/// Message the shell sends to the tray window for mouse activity on the icon.
const TRAY_MESSAGE: u32 = WM_APP + 3;
const TRAY_CLASS: &str = "tpt-shell-tray";
/// The notification-area id of the icon. Each tray has its own window, so one id is enough.
const ICON_ID: u32 = 1;

/// One item in the context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
}

struct State {
    items: Vec<MenuItem>,
    on_select: Box<dyn FnMut(u32)>,
}

/// An icon in the notification area. It is removed when dropped.
pub struct Tray {
    window: HWND,
    /// Owns the state the window procedure uses through `GWLP_USERDATA`.
    state: *mut State,
}

impl Tray {
    /// Adds the icon with `tooltip`. `on_select` gets [`ACTIVATE`] for a left click, or the id of
    /// the menu item picked. It runs on the UI thread, during the message loop.
    pub fn new(
        tooltip: &str,
        items: Vec<MenuItem>,
        on_select: impl FnMut(u32) + 'static,
    ) -> Result<Tray, String> {
        if items.iter().any(|item| item.id == ACTIVATE) {
            return Err("tray menu ids must be non-zero".into());
        }
        let state = Box::into_raw(Box::new(State {
            items,
            on_select: Box::new(on_select),
        }));
        let window = match create_window() {
            Ok(window) => window,
            Err(err) => {
                // SAFETY: created just above and never handed to anything else.
                drop(unsafe { Box::from_raw(state) });
                return Err(err);
            }
        };
        // SAFETY: the window is ours; GWLP_USERDATA holds the state until Drop reclaims it.
        unsafe { SetWindowLongPtrW(window, GWLP_USERDATA, state as isize) };
        // If the icon is refused, dropping `tray` removes the window and the state.
        let tray = Tray { window, state };
        tray.add_icon(tooltip)?;
        Ok(tray)
    }

    fn add_icon(&self, tooltip: &str) -> Result<(), String> {
        // SAFETY: NOTIFYICONDATAW is plain data, and every field the shell reads is set below.
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.window;
        nid.uID = ICON_ID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = TRAY_MESSAGE;
        // SAFETY: the default application icon is a system resource, so no module is needed.
        nid.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        let tip: Vec<u16> = tooltip.encode_utf16().take(127).collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        // SAFETY: `nid` is fully initialised and lives for the call.
        if unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } == 0 {
            return Err("the notification area did not accept the icon".into());
        }
        Ok(())
    }

    /// Sends the window a mouse message as if the shell had, for tests.
    #[cfg(test)]
    fn send_mouse(&self, mouse_message: u32) {
        // SAFETY: the window is alive while `self` is.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                self.window,
                TRAY_MESSAGE,
                ICON_ID as WPARAM,
                mouse_message as LPARAM,
            );
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.window;
        nid.uID = ICON_ID;
        // SAFETY: removes the icon this window added; the window is still alive.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
        // SAFETY: no messages reach the window after this, so nothing reads the state again.
        unsafe { DestroyWindow(self.window) };
        // SAFETY: `state` came from Box::into_raw in `new` and is reclaimed exactly once here.
        drop(unsafe { Box::from_raw(self.state) });
    }
}

fn create_window() -> Result<HWND, String> {
    let class = wide(TRAY_CLASS);
    // SAFETY: the module handle is for this executable.
    let hinstance = unsafe { GetModuleHandleW(null()) };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(tray_proc),
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
    // SAFETY: `wc` and the name live for the call. A failure means the class exists already.
    unsafe { RegisterClassExW(&wc) };
    // SAFETY: the class is registered; the window is message-only and never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            hinstance,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW (tray window) failed".into());
    }
    Ok(hwnd)
}

/// Window procedure of the tray window.
unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == TRAY_MESSAGE {
        let mouse = (lparam as u32) & 0xffff;
        match mouse {
            WM_LBUTTONUP => deliver(hwnd, ACTIVATE),
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                if let Some(id) = show_menu(hwnd) {
                    deliver(hwnd, id);
                }
            }
            _ => {}
        }
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Calls the tray's handler with `id`. A panic must not unwind across the window procedure.
fn deliver(hwnd: HWND, id: u32) {
    // SAFETY: GWLP_USERDATA holds the state for as long as the window exists, and the state is
    // only used on this thread, from the window procedure.
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if state.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as above; the reference does not outlive this call.
        let state = unsafe { &mut *state };
        (state.on_select)(id);
    }));
}

/// Shows the context menu at the cursor. Returns the id picked, if any.
fn show_menu(hwnd: HWND) -> Option<u32> {
    // SAFETY: the state is valid while the window procedure runs (see `deliver`).
    let items: Vec<MenuItem> = {
        let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
        if state.is_null() {
            return None;
        }
        unsafe { (*state).items.clone() }
    };
    // SAFETY: CreatePopupMenu takes no arguments and returns a menu we destroy below.
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return None;
    }
    for item in &items {
        let label = wide(&item.label);
        // SAFETY: the menu is ours, and the label is NUL-terminated for the call.
        unsafe { AppendMenuW(menu, MF_STRING, item.id as usize, label.as_ptr()) };
    }
    let mut at = POINT { x: 0, y: 0 };
    // SAFETY: `at` is a valid out-pointer.
    unsafe { GetCursorPos(&mut at) };
    // A menu only closes properly when its owner is the foreground window.
    // SAFETY: `hwnd` is the tray window.
    unsafe { SetForegroundWindow(hwnd) };
    // SAFETY: the menu and window are valid; TPM_RETURNCMD makes the call return the id picked.
    let picked = unsafe {
        TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            at.x,
            at.y,
            0,
            hwnd,
            null(),
        )
    };
    // SAFETY: the documented follow-up that stops the menu from sticking to the taskbar.
    unsafe { PostMessageW(hwnd, WM_NULL, 0, 0) };
    // SAFETY: the menu was created above and is not used after this.
    unsafe { DestroyMenu(menu) };
    (picked != 0).then_some(picked as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_menu_item_may_not_use_the_activate_id() {
        let items = vec![MenuItem {
            id: ACTIVATE,
            label: "bad".into(),
        }];
        assert!(Tray::new("tip", items, |_| {}).is_err());
    }

    #[test]
    fn a_left_click_reports_activate_on_the_ui_thread() {
        if !crate::window::interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        let seen: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let tray = match Tray::new(
            "tpt-shell test",
            vec![MenuItem {
                id: 7,
                label: "Open".into(),
            }],
            move |id| sink.lock().expect("sink").push(id),
        ) {
            Ok(tray) => tray,
            Err(err) => {
                eprintln!("skipped: the shell did not take the icon ({err})");
                return;
            }
        };
        tray.send_mouse(WM_LBUTTONUP);
        drop(tray);
        assert_eq!(seen.lock().expect("seen").as_slice(), &[ACTIVATE]);
    }
}
