// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Windows backend (spec §6.1): top-level windows, one WebView2 per window, the UI thread's
//! message loop, and per-monitor DPI.
//!
//! All state lives on the UI thread. Each window's WebView2 objects sit in a thread-local
//! registry keyed by the window handle, so the window procedure can reach them without
//! pointers into the backend. Every trait method checks that it runs on the UI thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::time::Duration;

use tpt_shell_core::backend::{
    AssetSource, Backdrop, CloseAction, EventLoopBackend, EventLoopProxy, MessageHandler,
    WebviewBackend, WindowBackend, WindowSpec,
};
use tpt_shell_core::theme::{injection_script, settings_script, Theme};
use tpt_shell_core::{Error, LogicalSize, PhysicalSize, Result, ScaleFactor};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
    GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowLongPtrW, GetWindowPlacement, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, SetWindowTextW,
    ShowWindow, TranslateMessage, CW_USEDEFAULT, GWL_STYLE, HWND_MESSAGE, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, MSG, PM_REMOVE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOW, WINDOWPLACEMENT, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_SETTINGCHANGE, WM_SIZE, WNDCLASSEXW, WS_CAPTION, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

use crate::com::{wide, HRESULT};
use crate::loader::OfficialLoader;
use crate::shim::SHIM_JS;
use crate::webview2::{create_environment, Controller, Environment, WebView, APP_ORIGIN};
use crate::window::{interactive_desktop_available, pump_until};

/// A task posted to the UI thread. See [`WinProxy`].
type Task = Box<dyn FnOnce() + Send>;

/// Window message that carries a boxed [`Task`] in `lparam`.
const WM_TASK: u32 = WM_APP + 1;
/// `DWMWA_SYSTEMBACKDROP_TYPE`, available on Windows 11 22H2 and later.
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
const WINDOW_CLASS: &str = "tpt_shell_window";
const CONTROLLER_TIMEOUT: Duration = Duration::from_secs(30);

/// Handle to a window managed by [`WindowsBackend`]. It is the window's `HWND` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowId(pub isize);

/// Saved placement and style while a window is fullscreen.
struct SavedPlacement {
    placement: WINDOWPLACEMENT,
    style: u32,
}

/// UI-thread state for one window.
struct Entry {
    controller: Controller,
    web: WebView,
    close_action: CloseAction,
    fullscreen: Option<SavedPlacement>,
    /// The owner window, for child windows.
    owner: Option<HWND>,
    /// Whether this child is modal. Closing it gives the owner its input back.
    modal: bool,
    /// The `data:` URL of the document this backend loaded, the only `data:` navigation allowed.
    host_document: Rc<RefCell<Option<String>>>,
    /// Receives messages the page posts. Set by `set_message_handler`; empty until then.
    page_handler: Rc<RefCell<Option<MessageHandler>>>,
    /// Answers requests for the app origin. Set by `set_asset_source`; empty until then.
    assets: Rc<RefCell<Option<AssetSource>>>,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<isize, Entry>> = RefCell::new(HashMap::new());
}

/// Whether `url` is the app origin or a path, query or fragment under it. A plain prefix
/// test would accept `https://tpt-app.localhost.example.com/`.
pub fn is_app_url(url: &str) -> bool {
    match url.strip_prefix(APP_ORIGIN) {
        Some(rest) => rest.is_empty() || rest.starts_with(['/', '?', '#']),
        None => false,
    }
}

/// Navigations the policy allows: the app origin, `about:blank` (used by WebView2 itself), the
/// exact `data:` URL of the document this backend loaded, and any URL the window's allowlist
/// covers (spec §9: remote navigation is blocked unless allowlisted per window).
fn allowed_navigation(url: &str, host_document: &Option<String>, allowlist: &[String]) -> bool {
    is_app_url(url)
        || url == "about:blank"
        || host_document.as_deref() == Some(url)
        || tpt_shell_core::navigation::url_is_allowlisted(url, allowlist)
}

/// The path under the app origin that `uri` names, or `None` if it is not a safe app path. An
/// empty path or `/` means `index.html`. A query or fragment is ignored. A `..` segment or a
/// backslash is refused, so a path cannot reach outside what the app supplies.
pub fn app_path(uri: &str) -> Option<String> {
    if !is_app_url(uri) {
        return None;
    }
    let rest = &uri[APP_ORIGIN.len()..];
    let path = rest
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    if path.is_empty() {
        return Some("index.html".into());
    }
    if path.contains('\\') || path.split('/').any(|segment| segment == "..") {
        return None;
    }
    Some(path.to_string())
}

/// The URL WebView2 uses when a document is loaded with NavigateToString.
fn html_data_url(html: &str) -> String {
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        base64_encode(html.as_bytes())
    )
}

/// Standard base64 with `=` padding.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(ALPHABET[(n >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

fn hr_error(what: &str, hr: HRESULT) -> Error {
    Error::Backend(format!("{what} failed (HRESULT 0x{hr:08X})"))
}

/// Cross-thread handle that posts tasks to the backend's message window.
#[derive(Clone, Copy)]
pub struct WinProxy {
    hwnd: isize,
}

impl EventLoopProxy for WinProxy {
    fn post(&self, task: Task) -> Result<()> {
        let raw = Box::into_raw(Box::new(task));
        // SAFETY: the window handle is only used for PostMessageW, which is thread-safe. The
        // pointer is owned by the message until the window procedure reclaims it.
        let ok = unsafe { PostMessageW(self.hwnd as HWND, WM_TASK, 0, raw as LPARAM) };
        if ok == 0 {
            // SAFETY: the message was not queued, so nothing else owns `raw`.
            drop(unsafe { Box::from_raw(raw) });
            return Err(Error::Backend("PostMessageW failed".into()));
        }
        Ok(())
    }
}

/// The Windows backend. Create it on the thread that will run the UI.
pub struct WindowsBackend {
    ui_thread: u32,
    com_initialized: bool,
    message_hwnd: HWND,
    quit_requested: bool,
    /// Applied to windows created after it is set.
    theme: Theme,
    // Declared before `_loader`, so it is released first; `Drop` takes it explicitly anyway.
    env: Option<Environment>,
    _loader: OfficialLoader,
}

impl WindowsBackend {
    /// Starts COM, loads the WebView2 loader from `loader_path`, and creates the environment
    /// in `user_data_dir`. Needs an interactive desktop and an installed WebView2 runtime.
    pub fn new(loader_path: &Path, user_data_dir: &Path) -> Result<Self> {
        if !interactive_desktop_available() {
            return Err(Error::Unsupported {
                feature: "windows backend without an interactive desktop",
            });
        }
        // SAFETY: standard apartment init for this thread; balanced in `Drop`.
        let hr = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err(hr_error("CoInitializeEx", hr));
        }
        match Self::start(loader_path, user_data_dir) {
            Ok((loader, env)) => {
                // SAFETY: no pointer arguments; per-monitor V2 is process-wide, and a second
                // call simply fails, which is fine.
                unsafe {
                    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                }
                let message_hwnd = match create_message_window() {
                    Ok(h) => h,
                    Err(e) => {
                        // SAFETY: balances the successful CoInitializeEx above.
                        unsafe { CoUninitialize() };
                        return Err(e);
                    }
                };
                // SAFETY: GetCurrentThreadId has no preconditions.
                let ui_thread = unsafe { GetCurrentThreadId() };
                Ok(WindowsBackend {
                    ui_thread,
                    com_initialized: true,
                    message_hwnd,
                    quit_requested: false,
                    theme: Theme::default(),
                    env: Some(env),
                    _loader: loader,
                })
            }
            Err(e) => {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
                Err(e)
            }
        }
    }

    fn start(loader_path: &Path, user_data_dir: &Path) -> Result<(OfficialLoader, Environment)> {
        let loader = OfficialLoader::load(loader_path)
            .map_err(|e| Error::Backend(format!("loader: {e}")))?;
        let create = loader
            .create_environment_fn()
            .map_err(|e| Error::Backend(format!("loader: {e}")))?;
        let slot: Rc<RefCell<Option<std::result::Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let done = slot.clone();
        create_environment(create, user_data_dir, move |r| *done.borrow_mut() = Some(r))
            .map_err(|hr| hr_error("CreateCoreWebView2Environment", hr))?;
        if !pump_until(CONTROLLER_TIMEOUT, || slot.borrow().is_some()) {
            return Err(Error::Backend("WebView2 environment timed out".into()));
        }
        let env = slot
            .borrow_mut()
            .take()
            .ok_or_else(|| Error::Backend("WebView2 environment lost".into()))?
            .map_err(|hr| hr_error("WebView2 environment", hr))?;
        Ok((loader, env))
    }

    /// Sets the theme for windows created after this call. Existing windows keep their theme.
    /// The theme is checked first; an invalid one is refused and nothing changes.
    pub fn set_theme(&mut self, theme: Theme) -> Result<()> {
        self.check_thread()?;
        theme.validate()?;
        self.theme = theme;
        Ok(())
    }

    /// Runs the message loop until [`EventLoopBackend::quit`] is called.
    pub fn run_until_quit(&mut self) -> Result<()> {
        self.check_thread()?;
        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: `msg` is a valid out-pointer; a null window takes every message.
            let got = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
            if got == 0 {
                return Ok(());
            }
            if got < 0 {
                return Err(Error::Backend("GetMessageW failed".into()));
            }
            // SAFETY: `msg` was just filled in by GetMessageW.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Title of the window's loaded document, or an empty string when none is loaded.
    pub fn document_title(&self, id: WindowId) -> Result<String> {
        self.with_web(id, |w| w.document_title())?
            .map_err(|hr| hr_error("document title", hr))
    }

    fn check_thread(&self) -> Result<()> {
        // SAFETY: GetCurrentThreadId has no preconditions.
        if unsafe { GetCurrentThreadId() } == self.ui_thread {
            Ok(())
        } else {
            Err(Error::Backend("called off the UI thread".into()))
        }
    }

    fn require(&self, id: WindowId) -> Result<HWND> {
        self.check_thread()?;
        let known = WINDOWS.with(|w| w.borrow().contains_key(&id.0));
        if known {
            Ok(id.0 as HWND)
        } else {
            Err(Error::UnknownWindow(id.0 as u64))
        }
    }

    fn with_entry<R>(&self, id: WindowId, f: impl FnOnce(&mut Entry) -> R) -> Result<R> {
        self.check_thread()?;
        WINDOWS.with(|w| {
            w.borrow_mut()
                .get_mut(&id.0)
                .map(f)
                .ok_or(Error::UnknownWindow(id.0 as u64))
        })
    }

    fn with_web<R>(&self, id: WindowId, f: impl FnOnce(&WebView) -> R) -> Result<R> {
        self.with_entry(id, |e| f(&e.web))
    }
}

impl Drop for WindowsBackend {
    fn drop(&mut self) {
        // Destroying a window sends WM_DESTROY, which closes its controller.
        let ids: Vec<isize> = WINDOWS.with(|w| w.borrow().keys().copied().collect());
        for id in ids {
            // SAFETY: the handle belongs to a window this backend created.
            unsafe { DestroyWindow(id as HWND) };
        }
        let leftovers: Vec<Entry> =
            WINDOWS.with(|w| w.borrow_mut().drain().map(|(_, e)| e).collect());
        for entry in leftovers {
            let _ = entry.controller.close();
        }
        // Free tasks that were posted but never run.
        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: `msg` is a valid out-pointer; the filter selects only our task messages.
            let found =
                unsafe { PeekMessageW(&mut msg, self.message_hwnd, WM_TASK, WM_TASK, PM_REMOVE) };
            if found == 0 {
                break;
            }
            // SAFETY: WM_TASK messages own a boxed task, reclaimed exactly once here.
            drop(unsafe { Box::from_raw(msg.lParam as *mut Task) });
        }
        // SAFETY: the message window was created by `new` and is destroyed once, here.
        unsafe { DestroyWindow(self.message_hwnd) };
        // Release the environment before COM is uninitialized.
        drop(self.env.take());
        if self.com_initialized {
            // SAFETY: balances the CoInitializeEx in `new`.
            unsafe { CoUninitialize() };
        }
    }
}

impl EventLoopBackend for WindowsBackend {
    type Proxy = WinProxy;

    fn proxy(&self) -> WinProxy {
        WinProxy {
            hwnd: self.message_hwnd as isize,
        }
    }

    fn quit(&mut self) {
        self.quit_requested = true;
        // SAFETY: PostQuitMessage has no pointer arguments.
        unsafe { PostQuitMessage(0) };
    }
}

impl WindowBackend for WindowsBackend {
    type Id = WindowId;

    fn create(&mut self, spec: &WindowSpec) -> Result<WindowId> {
        self.create_window(null_mut(), spec)
    }

    fn show(&mut self, id: WindowId) -> Result<()> {
        let hwnd = self.require(id)?;
        // SAFETY: `hwnd` is a live window registered with this backend.
        unsafe { ShowWindow(hwnd, SW_SHOW) };
        Ok(())
    }

    fn hide(&mut self, id: WindowId) -> Result<()> {
        let hwnd = self.require(id)?;
        // SAFETY: as in `show`.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        Ok(())
    }

    fn set_title(&mut self, id: WindowId, title: &str) -> Result<()> {
        let hwnd = self.require(id)?;
        let text = wide(title);
        // SAFETY: `hwnd` is live and `text` is NUL-terminated for the call.
        unsafe { SetWindowTextW(hwnd, text.as_ptr()) };
        Ok(())
    }

    fn set_size(&mut self, id: WindowId, size: LogicalSize) -> Result<()> {
        let hwnd = self.require(id)?;
        resize_logical(hwnd, size);
        Ok(())
    }

    fn set_fullscreen(&mut self, id: WindowId, on: bool) -> Result<()> {
        let hwnd = self.require(id)?;
        let already = self.with_entry(id, |e| e.fullscreen.is_some())?;
        if on == already {
            return Ok(());
        }
        if on {
            // SAFETY: `hwnd` is live; the out-structures are filled by the calls.
            unsafe {
                let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
                placement.length = size_of::<WINDOWPLACEMENT>() as u32;
                GetWindowPlacement(hwnd, &mut placement);
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let mut info: MONITORINFO = std::mem::zeroed();
                info.cbSize = size_of::<MONITORINFO>() as u32;
                let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                GetMonitorInfoW(monitor, &mut info);
                self.with_entry(id, |e| {
                    e.fullscreen = Some(SavedPlacement { placement, style });
                })?;
                SetWindowLongPtrW(
                    hwnd,
                    GWL_STYLE,
                    ((style & !WS_OVERLAPPEDWINDOW) | WS_POPUP) as isize,
                );
                let r = info.rcMonitor;
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE,
                );
            }
        } else {
            let saved = self.with_entry(id, |e| e.fullscreen.take())?;
            if let Some(saved) = saved {
                // SAFETY: `hwnd` is live and `saved.placement` came from GetWindowPlacement.
                unsafe {
                    SetWindowLongPtrW(hwnd, GWL_STYLE, saved.style as isize);
                    SetWindowPlacement(hwnd, &saved.placement);
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
                    );
                }
            }
        }
        Ok(())
    }

    fn close(&mut self, id: WindowId) -> Result<()> {
        let hwnd = self.require(id)?;
        // SAFETY: `hwnd` is live. WM_DESTROY removes the registry entry.
        unsafe { DestroyWindow(hwnd) };
        Ok(())
    }

    fn set_frameless(&mut self, id: WindowId, on: bool) -> Result<()> {
        let hwnd = self.require(id)?;
        // SAFETY: `hwnd` is live.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let new = if on {
                style & !WS_CAPTION
            } else {
                style | WS_CAPTION
            };
            SetWindowLongPtrW(hwnd, GWL_STYLE, new as isize);
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        }
        Ok(())
    }

    fn set_always_on_top(&mut self, id: WindowId, on: bool) -> Result<()> {
        let hwnd = self.require(id)?;
        let order = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: `hwnd` is live; only the z-order changes.
        let ok = unsafe {
            SetWindowPos(
                hwnd,
                order,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            // SAFETY: GetLastError has no preconditions.
            let code = unsafe { GetLastError() };
            return Err(Error::Backend(format!(
                "SetWindowPos failed (error {code})"
            )));
        }
        Ok(())
    }

    fn set_backdrop(&mut self, id: WindowId, backdrop: Backdrop) -> Result<()> {
        let hwnd = self.require(id)?;
        let value: i32 = match backdrop {
            Backdrop::None => 1,
            Backdrop::Mica => 2,
            Backdrop::Acrylic => 3,
            Backdrop::Tabbed => 4,
        };
        // SAFETY: `value` outlives the call and its size is passed exactly.
        let hr = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &value as *const i32 as *const c_void,
                size_of::<i32>() as u32,
            )
        };
        if hr < 0 {
            // Before Windows 11 22H2 the attribute does not exist.
            return Err(Error::Unsupported {
                feature: "backdrop",
            });
        }
        Ok(())
    }

    fn create_child(&mut self, parent: WindowId, spec: &WindowSpec) -> Result<WindowId> {
        let owner = self.require(parent)?;
        self.create_window(owner, spec)
    }

    fn set_modal(&mut self, child: WindowId, on: bool) -> Result<()> {
        let owner = self
            .with_entry(child, |e| e.owner)?
            .ok_or_else(|| Error::Backend("the window has no owner".into()))?;
        let changed = self.with_entry(child, |e| {
            let changed = e.modal != on;
            e.modal = on;
            changed
        })?;
        if changed {
            // SAFETY: `owner` was registered with this backend when the child was created;
            // EnableWindow on a handle that has since gone simply fails.
            unsafe { EnableWindow(owner, i32::from(!on)) };
        }
        Ok(())
    }

    fn set_close_action(&mut self, id: WindowId, action: CloseAction) -> Result<()> {
        self.with_entry(id, |e| e.close_action = action)
    }
}

impl WebviewBackend for WindowsBackend {
    fn set_message_handler(&mut self, id: WindowId, handler: MessageHandler) -> Result<()> {
        self.with_entry(id, |e| *e.page_handler.borrow_mut() = Some(handler))
    }

    fn set_asset_source(&mut self, id: WindowId, source: AssetSource) -> Result<()> {
        self.with_entry(id, |e| *e.assets.borrow_mut() = Some(source))
    }

    fn load_html(&mut self, id: WindowId, html: &str) -> Result<()> {
        self.with_entry(id, |e| {
            *e.host_document.borrow_mut() = Some(html_data_url(html));
            e.web.navigate_to_string(html)
        })?
        .map_err(|hr| hr_error("navigate_to_string", hr))
    }

    fn navigate(&mut self, id: WindowId, url: &str) -> Result<()> {
        if !is_app_url(url) {
            return Err(Error::Backend(format!("navigation blocked: {url}")));
        }
        self.with_web(id, |w| w.navigate(url))?
            .map_err(|hr| hr_error("navigate", hr))
    }

    /// Runs `script` in the page without waiting for it. Errors thrown by the script are not
    /// reported yet; only a failure to start the script is.
    fn eval(&mut self, id: WindowId, script: &str) -> Result<()> {
        self.with_web(id, |w| w.execute_script(script, |_hr| {}))?
            .map_err(|hr| hr_error("ExecuteScript", hr))
    }

    fn post_message(&mut self, id: WindowId, message: &str) -> Result<()> {
        self.with_web(id, |w| w.post_web_message(message))?
            .map_err(|hr| hr_error("post_web_message", hr))
    }
}

impl WindowsBackend {
    fn create_window(&mut self, owner: HWND, spec: &WindowSpec) -> Result<WindowId> {
        self.check_thread()?;
        let class = wide(WINDOW_CLASS);
        let title = wide(&spec.title);
        // SAFETY: the module handle is for this executable; the strings outlive the call.
        let hinstance = unsafe { GetModuleHandleW(null()) };
        // SAFETY: all pointer arguments are null or point at live NUL-terminated strings.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                owner,
                null_mut(),
                hinstance,
                null(),
            )
        };
        if hwnd.is_null() {
            return Err(Error::Backend("CreateWindowExW failed".into()));
        }
        let id = WindowId(hwnd as isize);
        // Size the window before the webview is attached, so the webview gets real bounds.
        resize_logical(hwnd, spec.size);
        match self.attach_webview(hwnd, &spec.allow_navigation) {
            Ok(mut entry) => {
                entry.owner = (!owner.is_null()).then_some(owner);
                WINDOWS.with(|w| w.borrow_mut().insert(id.0, entry));
                Ok(id)
            }
            Err(e) => {
                // SAFETY: the window was created above and is not registered yet.
                unsafe { DestroyWindow(hwnd) };
                Err(e)
            }
        }
    }

    /// Creates the controller and webview for a new window handle. The window stays hidden.
    fn attach_webview(&self, hwnd: HWND, allow_navigation: &[String]) -> Result<Entry> {
        let slot: Rc<RefCell<Option<std::result::Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let done = slot.clone();
        let env = self
            .env
            .as_ref()
            .ok_or_else(|| Error::Backend("environment released".into()))?;
        env.create_controller(hwnd as isize, move |r| *done.borrow_mut() = Some(r))
            .map_err(|hr| hr_error("CreateCoreWebView2Controller", hr))?;
        if !pump_until(CONTROLLER_TIMEOUT, || slot.borrow().is_some()) {
            return Err(Error::Backend("WebView2 controller timed out".into()));
        }
        let controller = slot
            .borrow_mut()
            .take()
            .ok_or_else(|| Error::Backend("WebView2 controller lost".into()))?
            .map_err(|hr| hr_error("WebView2 controller", hr))?;
        let web = controller
            .webview()
            .map_err(|hr| hr_error("get_CoreWebView2", hr))?;
        let page = page_settings(!cfg!(debug_assertions));
        let settings = web.settings().map_err(|hr| hr_error("get_Settings", hr))?;
        settings
            .set_devtools_enabled(page.devtools)
            .map_err(|hr| hr_error("put_AreDevToolsEnabled", hr))?;
        settings
            .set_default_context_menus_enabled(page.context_menu)
            .map_err(|hr| hr_error("put_AreDefaultContextMenusEnabled", hr))?;
        let host_document = Rc::new(RefCell::new(None::<String>));
        let page_handler = Rc::new(RefCell::new(None::<MessageHandler>));
        let for_page = page_handler.clone();
        web.on_web_message(move |sender, text| {
            let reply = match for_page.borrow_mut().as_mut() {
                Some(handler) => handler(text),
                None => None,
            };
            if let Some(reply) = reply {
                let _ = sender.post_web_message(&reply);
            }
        })
        .map_err(|hr| hr_error("add_WebMessageReceived", hr))?;
        // App pages come from the app origin. Requests for it are answered from the window's
        // asset source, and every answer carries the page policy (see `respond`).
        let assets: Rc<RefCell<Option<AssetSource>>> = Rc::new(RefCell::new(None));
        let for_assets = assets.clone();
        let serve_env = env.clone();
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .map_err(|hr| hr_error("AddWebResourceRequestedFilter", hr))?;
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let found = app_path(&uri).and_then(|path| {
                for_assets
                    .borrow()
                    .as_ref()
                    .and_then(|source| source(&path))
            });
            let _ = match found {
                Some(asset) => req.respond(&serve_env, 200, &asset.content_type, &asset.body),
                None => req.respond(&serve_env, 404, "text/plain", b"not found"),
            };
        })
        .map_err(|hr| hr_error("add_WebResourceRequested", hr))?;
        let for_top = host_document.clone();
        let allow_top = allow_navigation.to_vec();
        web.set_navigation_policy(move |url| {
            allowed_navigation(url, &for_top.borrow(), &allow_top)
        })
        .map_err(|hr| hr_error("NavigationStarting", hr))?;
        let for_frames = host_document.clone();
        let allow_frames = allow_navigation.to_vec();
        web.set_frame_navigation_policy(move |url| {
            allowed_navigation(url, &for_frames.borrow(), &allow_frames)
        })
        .map_err(|hr| hr_error("frame NavigationStarting", hr))?;
        let scheme = tpt_shell_core::ThemeBackend::color_scheme(self)?;
        let script = injection_script(&self.theme, scheme, crate::accessibility::current())?;
        web.add_document_start_script(&script, |_| {})
            .map_err(|hr| hr_error("AddScriptToExecuteOnDocumentCreated", hr))?;
        // The page runtime (`window.tpt`) goes in before any page script runs.
        web.add_document_start_script(SHIM_JS, |_| {})
            .map_err(|hr| hr_error("AddScriptToExecuteOnDocumentCreated", hr))?;
        let bounds =
            client_rect(hwnd).ok_or_else(|| Error::Backend("GetClientRect failed".into()))?;
        controller
            .set_bounds(bounds)
            .map_err(|hr| hr_error("put_Bounds", hr))?;
        Ok(Entry {
            controller,
            web,
            close_action: CloseAction::Close,
            fullscreen: None,
            owner: None,
            modal: false,
            host_document,
            page_handler,
            assets,
        })
    }
}

/// Switches a page window gets when it is created. Release builds turn the developer tools and
/// the right-click menu off (spec §9). Debug builds keep both, for development.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PageSettings {
    devtools: bool,
    context_menu: bool,
}

fn page_settings(release: bool) -> PageSettings {
    PageSettings {
        devtools: !release,
        context_menu: !release,
    }
}

/// Sets the window's outer size from logical units, using the window's current DPI.
fn resize_logical(hwnd: HWND, size: LogicalSize) {
    let PhysicalSize { width, height } = dpi_scale(hwnd).to_physical(size);
    // SAFETY: `hwnd` is a live window; the flags leave position and z-order alone.
    unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            width as i32,
            height as i32,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn dpi_scale(hwnd: HWND) -> ScaleFactor {
    // SAFETY: GetDpiForWindow accepts any handle and returns 0 for an invalid one.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    ScaleFactor::new(f64::from(dpi) / 96.0)
}

fn client_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    // SAFETY: `rect` is a valid out-pointer.
    let ok = unsafe { GetClientRect(hwnd, &mut rect) };
    (ok != 0).then_some(rect)
}

fn create_message_window() -> Result<HWND> {
    let class = wide(MESSAGE_CLASS);
    let title = wide("tpt-shell messages");
    // SAFETY: the module handle is for this executable; the class name is NUL-terminated.
    let hinstance = unsafe { GetModuleHandleW(null()) };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
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
    // SAFETY: `wc` and the strings it points to live for the call. A failure here means the
    // class exists already, and CreateWindowExW then reports the real error.
    unsafe { RegisterClassExW(&wc) };
    let window_class = wide(WINDOW_CLASS);
    let wc_app = WNDCLASSEXW {
        lpszClassName: window_class.as_ptr(),
        ..wc
    };
    // SAFETY: as above.
    unsafe { RegisterClassExW(&wc_app) };
    // SAFETY: all pointer arguments are null or point at live NUL-terminated strings.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
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
        return Err(Error::Backend(
            "CreateWindowExW (message window) failed".into(),
        ));
    }
    Ok(hwnd)
}

const MESSAGE_CLASS: &str = "tpt_shell_messages";

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TASK => {
            // SAFETY: WinProxy::post queued this pointer from Box::into_raw, once.
            let task: Box<Task> = Box::from_raw(lparam as *mut Task);
            let f: Task = *task;
            // A panic must not unwind across this extern function.
            let _ = catch_unwind(AssertUnwindSafe(f));
            0
        }
        WM_CLOSE => {
            let action = WINDOWS.with(|w| w.borrow().get(&(hwnd as isize)).map(|e| e.close_action));
            if action == Some(CloseAction::Hide) {
                ShowWindow(hwnd, SW_HIDE);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_SIZE => {
            if let Some(rect) = client_rect(hwnd) {
                WINDOWS.with(|w| {
                    if let Some(entry) = w.borrow().get(&(hwnd as isize)) {
                        let _ = entry.controller.set_bounds(rect);
                    }
                });
            }
            0
        }
        WM_SETTINGCHANGE => {
            // Sent for theme, colour and accessibility changes. Open pages follow them.
            push_settings_to_pages();
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DPICHANGED => {
            // SAFETY: on this message lParam points at the suggested new window rectangle.
            let r = &*(lparam as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            0
        }
        WM_DESTROY => {
            let entry = WINDOWS.with(|w| w.borrow_mut().remove(&(hwnd as isize)));
            if let Some(entry) = entry {
                // A modal child that goes away gives its owner back its input.
                if let (true, Some(owner)) = (entry.modal, entry.owner) {
                    EnableWindow(owner, 1);
                }
                let _ = entry.controller.close();
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl tpt_shell_core::ClipboardBackend for WindowsBackend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        self.check_thread()?;
        crate::clipboard::get_text().map_err(Error::Backend)
    }

    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        self.check_thread()?;
        crate::clipboard::set_text(text).map_err(Error::Backend)
    }
}

impl tpt_shell_core::ThemeBackend for WindowsBackend {
    fn color_scheme(&self) -> Result<tpt_shell_core::ColorScheme> {
        self.check_thread()?;
        let light = apps_use_light_theme().map_err(Error::Backend)?;
        Ok(if light {
            tpt_shell_core::ColorScheme::Light
        } else {
            tpt_shell_core::ColorScheme::Dark
        })
    }
}

/// Updates the colour scheme and accessibility attributes of every open page. A setting that
/// cannot be read leaves the pages as they are.
fn push_settings_to_pages() {
    let Ok(light) = apps_use_light_theme() else {
        return;
    };
    let scheme = if light {
        tpt_shell_core::ColorScheme::Light
    } else {
        tpt_shell_core::ColorScheme::Dark
    };
    let script = settings_script(scheme, crate::accessibility::current());
    WINDOWS.with(|w| {
        for entry in w.borrow().values() {
            let _ = entry.web.execute_script(&script, |_| {});
        }
    });
}

/// Reads the per-user "apps use light theme" setting. Returns `true` for light.
fn apps_use_light_theme() -> std::result::Result<bool, String> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut data: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: the strings are NUL-terminated and live for the call; `data` and `size` are
    // valid for the sizes given.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    if status != 0 {
        return Err(format!("reading the theme setting failed (error {status})"));
    }
    Ok(data != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"hi"), "aGk=");
    }

    #[test]
    fn host_data_url_matches_what_webview2_reports() {
        // The URL WebView2 reported for this document during a test run.
        assert_eq!(
            html_data_url("<!doctype html><title>tpt-backend-test</title><p>hi</p>"),
            "data:text/html;charset=utf-8;base64,PCFkb2N0eXBlIGh0bWw+PHRpdGxlPnRwdC1iYWNrZW5kLXRlc3Q8L3RpdGxlPjxwPmhpPC9wPg=="
        );
    }

    #[test]
    fn only_the_loaded_document_may_use_a_data_url() {
        let loaded = Some(html_data_url("<p>a</p>"));
        assert!(allowed_navigation(&html_data_url("<p>a</p>"), &loaded, &[]));
        assert!(!allowed_navigation(
            &html_data_url("<p>evil</p>"),
            &loaded,
            &[]
        ));
        assert!(!allowed_navigation(&html_data_url("<p>a</p>"), &None, &[]));
        assert!(allowed_navigation("about:blank", &None, &[]));
    }

    #[test]
    fn remote_pages_load_only_when_the_window_allowlists_them() {
        let list = vec!["https://docs.example.com".to_string()];
        assert!(!allowed_navigation(
            "https://docs.example.com/a",
            &None,
            &[]
        ));
        assert!(allowed_navigation(
            "https://docs.example.com/a",
            &None,
            &list
        ));
        assert!(!allowed_navigation(
            "https://docs.example.com.evil.net/",
            &None,
            &list
        ));
        assert!(!allowed_navigation(
            "https://other.example.com/",
            &None,
            &list
        ));
        assert!(!allowed_navigation(
            "http://docs.example.com/",
            &None,
            &list
        ));
    }

    #[test]
    fn app_urls_must_end_the_origin_at_a_boundary() {
        assert!(is_app_url(APP_ORIGIN));
        assert!(is_app_url("https://tpt-app.localhost/index.html"));
        assert!(is_app_url("https://tpt-app.localhost?x=1"));
        assert!(!is_app_url("https://tpt-app.localhost.example.com/"));
        assert!(!is_app_url("https://tpt-app.localhost@example.com/"));
        assert!(!is_app_url("https://example.com/"));
        assert!(!is_app_url("file:///C:/secret.txt"));
    }

    #[test]
    fn the_system_theme_setting_can_be_read() {
        // The registry value exists on every Windows 10 and 11 install. Only its success is
        // checked here, because the value depends on the machine.
        assert!(apps_use_light_theme().is_ok());
    }

    /// Needs `TPT_WEBVIEW2_LOADER` and an installed WebView2 runtime; skipped otherwise.
    mod runtime {
        use super::*;
        use crate::window::runtime_test_loader;
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        use std::sync::Arc;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsWindow, IsWindowVisible, PostQuitMessage, GWL_EXSTYLE, WM_CLOSE, WS_EX_TOPMOST,
        };

        fn data_dir(name: &str) -> std::path::PathBuf {
            std::env::temp_dir().join(format!("tpt-backend-{name}-{}", std::process::id()))
        }

        fn spec(name: &str) -> WindowSpec {
            WindowSpec {
                name: name.into(),
                title: name.into(),
                size: LogicalSize {
                    width: 640.0,
                    height: 480.0,
                },
                allow_navigation: Vec::new(),
            }
        }

        fn style_of(id: WindowId) -> u32 {
            // SAFETY: a read-only query on a window handle this backend created.
            unsafe { GetWindowLongPtrW(id.0 as HWND, GWL_STYLE) as u32 }
        }

        fn start(
            name: &str,
        ) -> Option<(
            WindowsBackend,
            std::path::PathBuf,
            std::sync::MutexGuard<'static, ()>,
        )> {
            let (loader, serial) = runtime_test_loader()?;
            let dir = data_dir(name);
            let backend = WindowsBackend::new(Path::new(&loader), &dir).expect("backend");
            Some((backend, dir, serial))
        }

        /// A page that asks for a file in scope, then one out of scope, and shows each reply in
        /// its title. The handler is the app's glue: a router, a grant and a consent prompt.
        const SCOPE_PAGE: &str = r#"<!doctype html><title>start</title><script>
        var IN_SCOPE = '{"v":1,"id":1,"cmd":"read","args":{"path":"C:/data/notes.txt"}}';
        var OUT_OF_SCOPE = '{"v":1,"id":2,"cmd":"read","args":{"path":"C:/secrets/key"}}';
        var answers = 0;
        window.chrome.webview.addEventListener('message', function (e) {
          answers++;
          document.title = (answers === 1 ? '' : document.title + ' | ') + e.data;
          if (answers === 1) { window.chrome.webview.postMessage(OUT_OF_SCOPE); }
        });
        window.chrome.webview.postMessage(IN_SCOPE);
        </script>"#;

        /// Answers consent prompts with no. The test grant needs none.
        struct Decline;

        impl tpt_shell_capabilities::ConsentPrompt for Decline {
            fn ask(&mut self, _window: &str, _capability: &str) -> bool {
                false
            }
        }

        #[test]
        fn page_messages_reach_the_capability_checked_router_and_the_reply_comes_back() {
            use std::cell::Cell;
            use std::rc::Rc;
            use tpt_shell_capabilities::{Engine, Grant, Scope};
            use tpt_shell_ipc::{Caller, Limits, Router, Target};

            let Some((mut b, dir, _serial)) = start("messages") else {
                return;
            };
            let reads = Rc::new(Cell::new(0u32));
            let counted = reads.clone();
            let mut router = Router::new();
            router.register_guarded(
                "read",
                "fs.read",
                |args| match args["path"].as_str() {
                    Some(path) => Target::Path(path.to_string()),
                    None => Target::None,
                },
                move |_| {
                    counted.set(counted.get() + 1);
                    Ok(tpt_shell_ipc::Value::String("contents".into()))
                },
            );
            let mut engine = Engine::new();
            engine
                .add_grant(Grant {
                    capability: "fs.read".into(),
                    window: "main".into(),
                    origin: APP_ORIGIN.into(),
                    scope: Scope::Paths(vec!["C:/data".into()]),
                    consent: false,
                })
                .expect("grant");
            let caller = Caller {
                window: "main".into(),
                origin: APP_ORIGIN.into(),
            };
            let mut prompt = Decline;

            let id = b.create(&spec("main")).expect("create");
            b.show(id).expect("show");
            b.set_message_handler(
                id,
                Box::new(move |text| {
                    Some(router.handle_authorized(
                        &caller,
                        &mut engine,
                        &mut prompt,
                        text.as_bytes(),
                        &Limits::default(),
                    ))
                }),
            )
            .expect("set_message_handler");
            b.load_html(id, SCOPE_PAGE).expect("load_html");

            let mut title = String::new();
            let got = pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title.contains("CAP_OUT_OF_SCOPE")
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "the second reply never arrived; title was {title:?}");
            let (first, second) = title.split_once(" | ").expect("two replies");
            assert!(first.contains(r#""ok":true"#), "in-scope call: {first}");
            assert!(
                first.contains(r#""data":"contents""#),
                "in-scope call: {first}"
            );
            assert!(
                second.contains(r#""code":"CAP_OUT_OF_SCOPE""#),
                "out of scope: {second}"
            );
            assert_eq!(
                reads.get(),
                1,
                "the handler runs for the in-scope call only"
            );
        }

        #[test]
        fn app_pages_load_from_the_app_origin_with_their_own_scripts() {
            use tpt_shell_core::backend::Asset;
            let Some((mut b, dir, _serial)) = start("assets") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            b.show(id).expect("show");
            b.set_asset_source(
                id,
                Box::new(|path| {
                    let (content_type, body) = match path {
                        "index.html" => (
                            "text/html",
                            r#"<!doctype html><title>start</title><script src="app.js"></script>"#,
                        ),
                        "app.js" => ("text/javascript", "document.title = location.origin;"),
                        _ => return None,
                    };
                    Some(Asset {
                        content_type: content_type.into(),
                        body: body.as_bytes().to_vec(),
                    })
                }),
            )
            .expect("set_asset_source");
            b.navigate(id, &format!("{APP_ORIGIN}/index.html"))
                .expect("navigate");
            let mut title = String::new();
            let got = pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title == APP_ORIGIN
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "title was {title:?}, expected the app origin");
        }

        #[test]
        fn creates_a_window_and_renders_html() {
            let Some((mut b, dir, _serial)) = start("render") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            b.show(id).expect("show");
            b.load_html(
                id,
                "<!doctype html><title>tpt-backend-test</title><p>hi</p>",
            )
            .expect("load_html");
            let mut title = String::new();
            let got = pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title == "tpt-backend-test"
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "title never arrived; last was {title:?}");
        }

        #[test]
        fn eval_runs_script_in_the_page() {
            let Some((mut b, dir, _serial)) = start("eval") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            b.load_html(id, "<!doctype html><title>before</title>")
                .expect("load_html");
            let mut title = String::new();
            assert!(pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title == "before"
            }));
            b.eval(id, "document.title = 'after'").expect("eval");
            let got = pump_until(Duration::from_secs(10), || {
                title = b.document_title(id).unwrap_or_default();
                title == "after"
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "script did not change the title; last was {title:?}");
        }

        #[test]
        fn theme_is_applied_to_new_pages() {
            let Some((mut b, dir, _serial)) = start("theme") else {
                return;
            };
            b.set_theme(Theme {
                accent: "#123456".into(),
                ..Theme::default()
            })
            .expect("set theme");
            let scheme = match tpt_shell_core::ThemeBackend::color_scheme(&b).expect("scheme") {
                tpt_shell_core::ColorScheme::Light => "light",
                tpt_shell_core::ColorScheme::Dark => "dark",
            };
            let id = b.create(&spec("main")).expect("create");
            b.load_html(
                id,
                "<!doctype html><title>x</title><script>document.addEventListener('DOMContentLoaded', function () { document.title = document.documentElement.dataset.colorScheme + '|' + document.documentElement.style.getPropertyValue('--tpt-accent').trim(); });</script>",
            )
            .expect("load_html");
            let expected = format!("{scheme}|#123456");
            let mut title = String::new();
            let got = pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title == expected
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "expected title {expected:?}, last was {title:?}");
        }

        #[test]
        fn accessibility_signals_and_the_page_runtime_reach_the_page() {
            let Some((mut b, dir, _serial)) = start("signals") else {
                return;
            };
            let signals = crate::accessibility::current();
            let id = b.create(&spec("main")).expect("create");
            b.load_html(
                id,
                "<!doctype html><title>x</title><script>document.addEventListener('DOMContentLoaded', function () { var d = document.documentElement.dataset; document.title = [d.highContrast, d.reducedMotion, d.reducedTransparency, typeof window.tpt.on, window.tpt.caps.engine].join('|'); });</script>",
            )
            .expect("load_html");
            let expected = format!(
                "{}|{}|{}|function|webview2",
                signals.high_contrast, signals.reduced_motion, signals.reduced_transparency
            );
            let mut title = String::new();
            let got = pump_until(Duration::from_secs(30), || {
                title = b.document_title(id).unwrap_or_default();
                title == expected
            });
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "expected title {expected:?}, last was {title:?}");
        }

        #[test]
        fn open_pages_follow_a_settings_change() {
            let Some((mut b, dir, _serial)) = start("settings-push") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            b.load_html(
                id,
                "<!doctype html><title>x</title><script>document.addEventListener('DOMContentLoaded', function () { document.title = 'ready'; });</script>",
            )
            .expect("load_html");
            let mut title = String::new();
            let mut wait_for = |b: &WindowsBackend, want: &str| {
                pump_until(Duration::from_secs(30), || {
                    title = b.document_title(id).unwrap_or_default();
                    title == want
                })
            };
            assert!(
                wait_for(&b, "ready"),
                "page never loaded; title was {title:?}"
            );

            // Spoil the attribute, then push the real settings and read them back.
            b.eval(id, "document.documentElement.dataset.colorScheme = 'bogus'; document.title = 'spoiled';")
                .expect("spoil");
            assert!(
                wait_for(&b, "spoiled"),
                "spoil did not run; title was {title:?}"
            );
            push_settings_to_pages();
            b.eval(
                id,
                "document.title = document.documentElement.dataset.colorScheme + '|' + document.documentElement.dataset.highContrast;",
            )
            .expect("read back");

            let light = apps_use_light_theme().expect("theme setting");
            let signals = crate::accessibility::current();
            let scheme = if light { "light" } else { "dark" };
            let expected = format!("{scheme}|{}", signals.high_contrast);
            let got = wait_for(&b, &expected);
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
            assert!(got, "expected title {expected:?}, last was {title:?}");
        }

        #[test]
        fn an_invalid_theme_is_refused_and_the_old_one_stays() {
            let Some((mut b, dir, _serial)) = start("bad-theme") else {
                return;
            };
            let bad = Theme {
                accent: "javascript:".into(),
                ..Theme::default()
            };
            assert!(b.set_theme(bad).is_err());
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn modal_child_disables_its_owner_until_it_closes() {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
            let Some((mut b, dir, _serial)) = start("modal") else {
                return;
            };
            let main = b.create(&spec("main")).expect("main");
            let dialog = b.create_child(main, &spec("dialog")).expect("child");
            b.show(main).expect("show main");
            b.show(dialog).expect("show child");
            let enabled = |id: WindowId| unsafe { IsWindowEnabled(id.0 as HWND) } != 0;

            assert!(enabled(main));
            b.set_modal(dialog, true).expect("modal on");
            assert!(
                !enabled(main),
                "owner is disabled while the modal child is open"
            );
            b.set_modal(dialog, false).expect("modal off");
            assert!(enabled(main));

            b.set_modal(dialog, true).expect("modal on again");
            b.close(dialog).expect("close child");
            pump_until(Duration::from_secs(5), || false);
            assert!(
                enabled(main),
                "closing a modal child gives the owner its input back"
            );
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn release_builds_turn_devtools_and_the_context_menu_off() {
            assert_eq!(
                page_settings(true),
                PageSettings {
                    devtools: false,
                    context_menu: false
                }
            );
            assert_eq!(
                page_settings(false),
                PageSettings {
                    devtools: true,
                    context_menu: true
                }
            );
        }

        #[test]
        fn navigation_outside_the_app_origin_is_refused() {
            let Some((mut b, dir, _serial)) = start("nav") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            assert!(b.navigate(id, "https://example.com/").is_err());
            assert!(b.navigate(id, "file:///C:/Windows/win.ini").is_err());
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn frameless_topmost_and_fullscreen_change_the_window_state() {
            let Some((mut b, dir, _serial)) = start("chrome") else {
                return;
            };
            let id = b.create(&spec("main")).expect("create");
            let hwnd = id.0 as HWND;

            assert_ne!(style_of(id) & WS_CAPTION, 0, "a new window has a caption");
            b.set_frameless(id, true).expect("frameless on");
            assert_eq!(
                style_of(id) & WS_CAPTION,
                0,
                "frameless removes the caption"
            );
            b.set_frameless(id, false).expect("frameless off");
            assert_ne!(style_of(id) & WS_CAPTION, 0);

            // Topmost is checked on a visible window: that is the case the user cares about.
            b.show(id).expect("show");
            b.set_always_on_top(id, true).expect("topmost on");
            // SAFETY: read-only query on a live handle.
            let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
            assert_ne!(
                ex & WS_EX_TOPMOST,
                0,
                "topmost flag, extended style 0x{ex:08X}"
            );
            b.set_always_on_top(id, false).expect("topmost off");

            b.set_fullscreen(id, true).expect("fullscreen on");
            assert_ne!(style_of(id) & WS_POPUP, 0, "fullscreen uses a popup style");
            b.set_fullscreen(id, false).expect("fullscreen off");
            assert_ne!(
                style_of(id) & WS_CAPTION,
                0,
                "leaving fullscreen restores the caption"
            );

            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn close_hide_keeps_the_window_and_default_close_destroys_it() {
            let Some((mut b, dir, _serial)) = start("close") else {
                return;
            };
            let kept = b.create(&spec("kept")).expect("create kept");
            b.show(kept).expect("show");
            b.set_close_action(kept, CloseAction::Hide)
                .expect("set action");
            // SAFETY: posting WM_CLOSE to a live window is the same as a user close request.
            unsafe { PostMessageW(kept.0 as HWND, WM_CLOSE, 0, 0) };
            pump_until(Duration::from_millis(300), || false);
            // SAFETY: read-only queries on the handle.
            unsafe {
                assert_ne!(IsWindow(kept.0 as HWND), 0, "hide keeps the window");
                assert_eq!(IsWindowVisible(kept.0 as HWND), 0, "hide hides it");
            }

            let gone = b.create(&spec("gone")).expect("create gone");
            // SAFETY: as above.
            unsafe { PostMessageW(gone.0 as HWND, WM_CLOSE, 0, 0) };
            let destroyed = pump_until(Duration::from_secs(5), || {
                // SAFETY: read-only query; the handle may be stale, which IsWindow handles.
                unsafe { IsWindow(gone.0 as HWND) == 0 }
            });
            assert!(destroyed, "default close should destroy the window");
            assert!(matches!(b.show(gone), Err(Error::UnknownWindow(_))));

            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn proxy_tasks_run_on_the_ui_thread() {
            let Some((b, dir, _serial)) = start("proxy") else {
                return;
            };
            let ui = unsafe { GetCurrentThreadId() };
            let proxy = b.proxy();
            let ran = Arc::new(AtomicBool::new(false));
            let ran_on = Arc::new(AtomicU32::new(0));
            let (r, t) = (ran.clone(), ran_on.clone());
            std::thread::spawn(move || {
                proxy
                    .post(Box::new(move || {
                        // SAFETY: GetCurrentThreadId has no preconditions.
                        t.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
                        r.store(true, Ordering::SeqCst);
                    }))
                    .expect("post");
            })
            .join()
            .unwrap();
            let got = pump_until(Duration::from_secs(10), || ran.load(Ordering::SeqCst));
            assert!(got, "posted task never ran");
            assert_eq!(ran_on.load(Ordering::SeqCst), ui);
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn quit_ends_the_message_loop() {
            let Some((mut b, dir, _serial)) = start("quit") else {
                return;
            };
            let proxy = b.proxy();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                let _ = proxy.post(Box::new(|| {
                    // SAFETY: PostQuitMessage has no pointer arguments.
                    unsafe { PostQuitMessage(0) };
                }));
            });
            b.run_until_quit().expect("loop returns on WM_QUIT");
            drop(b);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod app_path_tests {
    use super::*;

    #[test]
    fn app_paths_map_onto_the_asset_names() {
        assert_eq!(app_path(APP_ORIGIN).as_deref(), Some("index.html"));
        assert_eq!(
            app_path(&format!("{APP_ORIGIN}/")).as_deref(),
            Some("index.html")
        );
        assert_eq!(
            app_path(&format!("{APP_ORIGIN}/app.js")).as_deref(),
            Some("app.js")
        );
        assert_eq!(
            app_path(&format!("{APP_ORIGIN}/app.js?v=2#top")).as_deref(),
            Some("app.js")
        );
        assert_eq!(
            app_path(&format!("{APP_ORIGIN}/css/site.css")).as_deref(),
            Some("css/site.css")
        );
    }

    #[test]
    fn paths_that_leave_the_app_or_its_origin_are_refused() {
        assert_eq!(app_path(&format!("{APP_ORIGIN}/../secret")), None);
        assert_eq!(app_path(&format!("{APP_ORIGIN}/a/../../secret")), None);
        assert_eq!(app_path(&format!("{APP_ORIGIN}/a\\b")), None);
        assert_eq!(app_path("https://tpt-app.localhost.evil.net/app.js"), None);
        assert_eq!(app_path("https://example.com/app.js"), None);
    }
}
