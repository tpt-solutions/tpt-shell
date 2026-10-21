// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! WebView2 environment, controller and webview, through hand-written vtables (RFC 0001).
//!
//! Each vtable declares only the slots we call. Slots before them are placeholders sized
//! from the header, so slot positions match `WebView2.h`. The header test below checks that.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, RECT};

use windows_sys::Win32::UI::Shell::SHCreateMemStream;

use crate::com::{wide, ComPtr, Guid, IUnknownVtbl, E_POINTER, HRESULT};
use crate::event_handler::new_event_handler;
use crate::handler::{new_completed_handler, release};
use crate::loader::CreateEnvironmentFn;

pub const IID_ENVIRONMENT_COMPLETED_HANDLER: Guid = Guid {
    data1: 0x4e8a3389,
    data2: 0xc9d8,
    data3: 0x4bd2,
    data4: [0xb6, 0xb5, 0x12, 0x4f, 0xee, 0x6c, 0xc1, 0x4d],
};
pub const IID_CONTROLLER_COMPLETED_HANDLER: Guid = Guid {
    data1: 0x6c4819f3,
    data2: 0xc9b7,
    data3: 0x4260,
    data4: [0x81, 0x27, 0xc9, 0xf5, 0xbd, 0xe7, 0xf6, 0x8c],
};
pub const IID_WEB_RESOURCE_REQUESTED_HANDLER: Guid = Guid {
    data1: 0xab00b74c,
    data2: 0x15f1,
    data3: 0x4646,
    data4: [0x80, 0xe8, 0xe7, 0x63, 0x41, 0xd2, 0x5d, 0x71],
};
pub const IID_SCRIPT_ADDED_HANDLER: Guid = Guid {
    data1: 0xb99369f3,
    data2: 0x9b11,
    data3: 0x47b5,
    data4: [0xbc, 0x6f, 0x8e, 0x78, 0x95, 0xfc, 0xea, 0x17],
};
pub const IID_PROCESS_FAILED_HANDLER: Guid = Guid {
    data1: 0x79e0aea4,
    data2: 0x990b,
    data3: 0x42d9,
    data4: [0xaa, 0x1d, 0x0f, 0xcc, 0x2e, 0x5b, 0xc7, 0xf1],
};
pub const IID_DEVTOOLS_DONE_HANDLER: Guid = Guid {
    data1: 0x5c4889f0,
    data2: 0x5ef6,
    data3: 0x4c5a,
    data4: [0x95, 0x2c, 0xd8, 0xf1, 0xb9, 0x2d, 0x05, 0x74],
};
pub const IID_NAVIGATION_STARTING_HANDLER: Guid = Guid {
    data1: 0x9adbe429,
    data2: 0xf36d,
    data3: 0x432b,
    data4: [0x9d, 0xdc, 0xf8, 0x88, 0x1f, 0xbd, 0x76, 0xe3],
};
pub const IID_EXECUTE_SCRIPT_HANDLER: Guid = Guid {
    data1: 0x49511172,
    data2: 0xcc67,
    data3: 0x4bca,
    data4: [0x99, 0x23, 0x13, 0x71, 0x12, 0xf4, 0xc4, 0xcc],
};
pub const IID_WEB_MESSAGE_RECEIVED_HANDLER: Guid = Guid {
    data1: 0x57213f19,
    data2: 0x00e6,
    data3: 0x49fa,
    data4: [0x8e, 0x07, 0x89, 0x8e, 0xa0, 0x1e, 0xcb, 0xd2],
};

/// Origin the app's own pages are served from on Windows. WebView2 does not route a custom
/// scheme such as `app://` through `WebResourceRequested` (checked against runtime 155.0.4283.62),
/// so pages use a virtual `https` host that we answer ourselves. See the RFC 0001 status note.
pub const APP_ORIGIN: &str = "https://tpt-app.localhost";

// Header indexes of the slots used below:
//   ICoreWebView2Environment::CreateCoreWebView2Controller = 0
//   ICoreWebView2Controller::Close = 21, get_CoreWebView2 = 22
//   ICoreWebView2::Navigate = 2, NavigateToString = 3, get_DocumentTitle = 45

#[repr(C)]
struct EnvironmentVtbl {
    base: IUnknownVtbl,
    create_controller: unsafe extern "system" fn(*mut c_void, HWND, *mut c_void) -> HRESULT,
    create_web_resource_response: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        i32,
        *const u16,
        *const u16,
        *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(C)]
struct ControllerVtbl {
    base: IUnknownVtbl,
    _before_bounds: [usize; 3],
    put_bounds: unsafe extern "system" fn(*mut c_void, RECT) -> HRESULT,
    _before_close: [usize; 17],
    close: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_core_webview2: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

/// `ICoreWebView2Settings`. The slots we use: get_AreDevToolsEnabled = 8, put_AreDevToolsEnabled
/// = 9, put_AreDefaultContextMenusEnabled = 11 (BOOL is an i32 in the vtable).
#[repr(C)]
struct SettingsVtbl {
    base: IUnknownVtbl,
    _before_devtools_get: [usize; 8],
    get_devtools_enabled: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
    put_devtools_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
    _before_context_menus: [usize; 1],
    put_default_context_menus_enabled: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
}

#[repr(C)]
struct WebViewVtbl {
    base: IUnknownVtbl,
    get_settings: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    _before_navigate: [usize; 1],
    navigate: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    navigate_to_string: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    add_navigation_starting:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_frame_navigation: [usize; 9],
    add_frame_navigation_starting:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_script: [usize; 7],
    add_process_failed: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_process_failed_remove: [usize; 1],
    add_script_to_execute_on_document_created:
        unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> HRESULT,
    // Removes the script added to document creation (vtable 28), then ExecuteScript (29).
    _before_execute_script: [usize; 1],
    execute_script: unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> HRESULT,
    // CapturePreview (30), then Reload (31).
    _before_reload: [usize; 1],
    reload: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    _before_post_message: [usize; 1],
    post_web_message_as_string: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    add_web_message_received:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_devtools: [usize; 1],
    call_devtools_protocol_method:
        unsafe extern "system" fn(*mut c_void, *const u16, *const u16, *mut c_void) -> HRESULT,
    _before_document_title: [usize; 11],
    get_document_title: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
    _before_resource_requested: [usize; 6],
    add_web_resource_requested:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_resource_filter: [usize; 1],
    add_web_resource_requested_filter:
        unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT,
}

/// `ICoreWebView2WebResourceRequestedEventArgs`: get_Request = 0, put_Response = 2.
#[repr(C)]
struct WebResourceArgsVtbl {
    base: IUnknownVtbl,
    get_request: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    _get_response: usize,
    put_response: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

/// `ICoreWebView2WebResourceRequest`: get_Uri = 0.
#[repr(C)]
struct WebResourceRequestVtbl {
    base: IUnknownVtbl,
    get_uri: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
}

/// `ICoreWebView2ProcessFailedEventArgs`: get_ProcessFailedKind = 0.
#[repr(C)]
struct ProcessFailedArgsVtbl {
    base: IUnknownVtbl,
    get_process_failed_kind: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

/// `ICoreWebView2NavigationStartingEventArgs`: get_Uri = 0, put_Cancel = 5.
#[repr(C)]
struct NavigationArgsVtbl {
    base: IUnknownVtbl,
    get_uri: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
    _before_cancel: [usize; 4],
    put_cancel: unsafe extern "system" fn(*mut c_void, i32) -> HRESULT,
}

/// `ICoreWebView2WebMessageReceivedEventArgs`: TryGetWebMessageAsString = 2.
#[repr(C)]
struct WebMessageArgsVtbl {
    base: IUnknownVtbl,
    _get_source: usize,
    _get_json: usize,
    try_get_web_message_as_string: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
}

/// Content-Security-Policy for every response the shell serves (spec §9). Only the app's own
/// scripts and styles run: no inline script or style, no remote origins, no plugins.
pub const PAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

/// Header block for a served response: its content type and the page policy.
fn response_headers(content_type: &str) -> String {
    format!("Content-Type: {content_type}\r\nContent-Security-Policy: {PAGE_CSP}\r\n")
}

/// A request the webview wants to make, which we can answer (for `app://` content).
pub struct WebResourceRequest(ComPtr);

impl WebResourceRequest {
    pub fn uri(&self) -> Result<String, HRESULT> {
        let args: &WebResourceArgsVtbl = self.0.vtbl();
        let mut request: *mut c_void = null_mut();
        // SAFETY: `request` is a valid out-pointer.
        let hr = unsafe { (args.get_request)(self.0.as_ptr(), &mut request) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success `request` is a new reference that we own.
        let request = unsafe { ComPtr::from_owned(request) }.ok_or(E_POINTER)?;
        let vt: &WebResourceRequestVtbl = request.vtbl();
        let mut out: *mut u16 = null_mut();
        // SAFETY: `out` is a valid out-pointer for a COM-allocated string.
        let hr = unsafe { (vt.get_uri)(request.as_ptr(), &mut out) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success `out` is a COM-allocated, NUL-terminated string.
        Ok(unsafe { crate::com::take_co_string(out) })
    }

    /// Answers the request with `body`. `env` creates the response object.
    pub fn respond(
        &self,
        env: &Environment,
        status: i32,
        content_type: &str,
        body: &[u8],
    ) -> Result<(), HRESULT> {
        // SAFETY: `body` is readable for `body.len()` bytes; the stream copies them.
        let stream = unsafe { SHCreateMemStream(body.as_ptr(), body.len() as u32) };
        // SAFETY: on success the call returns a new reference to the stream.
        let stream = unsafe { ComPtr::from_owned(stream) }.ok_or(E_POINTER)?;
        let reason = wide(if status == 200 { "OK" } else { "Not Found" });
        let headers = wide(&response_headers(content_type));
        let env_vt: &EnvironmentVtbl = env.0.vtbl();
        let mut response: *mut c_void = null_mut();
        // SAFETY: the stream, strings and out-pointer are valid for the call.
        let hr = unsafe {
            (env_vt.create_web_resource_response)(
                env.0.as_ptr(),
                stream.as_ptr(),
                status,
                reason.as_ptr(),
                headers.as_ptr(),
                &mut response,
            )
        };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success `response` is a new reference that we own.
        let response = unsafe { ComPtr::from_owned(response) }.ok_or(E_POINTER)?;
        let args: &WebResourceArgsVtbl = self.0.vtbl();
        // SAFETY: `response` is a live response object for the duration of the call.
        let hr = unsafe { (args.put_response)(self.0.as_ptr(), response.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }
}

/// Why a WebView2 process failed (`COREWEBVIEW2_PROCESS_FAILED_KIND`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessFailure {
    /// The browser process exited. The whole webview is gone.
    BrowserExited,
    /// A renderer exited. The page is gone; reloading it usually recovers the page.
    RenderExited,
    /// A kind this version does not name, with its raw value.
    Other(i32),
}

impl ProcessFailure {
    fn from_kind(kind: i32) -> Self {
        match kind {
            0 => ProcessFailure::BrowserExited,
            1 => ProcessFailure::RenderExited,
            other => ProcessFailure::Other(other),
        }
    }
}

/// `ICoreWebView2Environment`.
pub struct Environment(ComPtr);

/// `ICoreWebView2Controller`.
pub struct Controller(ComPtr);

/// `ICoreWebView2`.
pub struct WebView(ComPtr);

/// `ICoreWebView2Settings`: switches that apply to every page the webview shows.
pub struct Settings(ComPtr);

/// Default WebView2 user-data folder for an app (RFC 0001, question 2):
/// `<local_app_data>\tpt-shell\<app_id>\webview2`. `app_id` must be one plain folder name:
/// letters, digits, `-`, `_` or `.`, not `.` or `..`.
pub fn default_user_data_dir(local_app_data: &Path, app_id: &str) -> Result<PathBuf, String> {
    let plain = !app_id.is_empty()
        && app_id != "."
        && app_id != ".."
        && app_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !plain {
        return Err(format!(
            "invalid app id `{app_id}`: use one plain folder name"
        ));
    }
    Ok(local_app_data
        .join("tpt-shell")
        .join(app_id)
        .join("webview2"))
}

/// Starts environment creation. `on_done` runs on the UI thread, from the message pump.
pub fn create_environment(
    create: CreateEnvironmentFn,
    user_data_dir: &Path,
    on_done: impl FnOnce(Result<Environment, HRESULT>) + 'static,
) -> Result<(), HRESULT> {
    let user_data = wide(&user_data_dir.to_string_lossy());
    let handler = new_completed_handler(IID_ENVIRONMENT_COMPLETED_HANDLER, move |hr, obj| {
        let result = if hr < 0 {
            Err(hr)
        } else {
            // SAFETY: on success the runtime passes a live environment pointer.
            unsafe { ComPtr::from_borrowed(obj) }
                .map(Environment)
                .ok_or(E_POINTER)
        };
        on_done(result);
    });
    // SAFETY: a null browser folder selects the installed Evergreen runtime, a null options
    // pointer selects defaults, and `user_data` and `handler` are valid for the call.
    let hr = unsafe { create(std::ptr::null(), user_data.as_ptr(), null_mut(), handler) };
    // SAFETY: we release our own reference. A runtime that keeps the handler took its own.
    unsafe { release(handler) };
    if hr < 0 {
        Err(hr)
    } else {
        Ok(())
    }
}

impl Environment {
    /// Starts controller creation on the window whose handle value is `parent`.
    /// `on_done` runs on the UI thread.
    pub fn create_controller(
        &self,
        parent: isize,
        on_done: impl FnOnce(Result<Controller, HRESULT>) + 'static,
    ) -> Result<(), HRESULT> {
        let handler = new_completed_handler(IID_CONTROLLER_COMPLETED_HANDLER, move |hr, obj| {
            let result = if hr < 0 {
                Err(hr)
            } else {
                // SAFETY: on success the runtime passes a live controller pointer.
                unsafe { ComPtr::from_borrowed(obj) }
                    .map(Controller)
                    .ok_or(E_POINTER)
            };
            on_done(result);
        });
        let vt: &EnvironmentVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live environment and `handler` is a live handler.
        let hr = unsafe { (vt.create_controller)(self.0.as_ptr(), parent as HWND, handler) };
        // SAFETY: as in `create_environment`.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }
}

impl Settings {
    /// Turns the developer tools on or off. Off also blocks F12 and the DevTools shortcuts.
    pub fn set_devtools_enabled(&self, on: bool) -> Result<(), HRESULT> {
        let vt: &SettingsVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live settings object.
        check(unsafe { (vt.put_devtools_enabled)(self.0.as_ptr(), i32::from(on)) })
    }

    /// Reads back whether the developer tools are on.
    pub fn devtools_enabled(&self) -> Result<bool, HRESULT> {
        let vt: &SettingsVtbl = self.0.vtbl();
        let mut on: i32 = 0;
        // SAFETY: `on` is a valid out-pointer for a BOOL.
        check(unsafe { (vt.get_devtools_enabled)(self.0.as_ptr(), &mut on) })?;
        Ok(on != 0)
    }

    /// Turns the default right-click menu on or off.
    pub fn set_default_context_menus_enabled(&self, on: bool) -> Result<(), HRESULT> {
        let vt: &SettingsVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live settings object.
        check(unsafe { (vt.put_default_context_menus_enabled)(self.0.as_ptr(), i32::from(on)) })
    }
}

/// Maps a failed HRESULT to `Err`.
fn check(hr: HRESULT) -> Result<(), HRESULT> {
    if hr < 0 {
        Err(hr)
    } else {
        Ok(())
    }
}

impl Controller {
    /// Sets the area the webview fills, in the parent window's client coordinates.
    pub fn set_bounds(&self, bounds: RECT) -> Result<(), HRESULT> {
        let vt: &ControllerVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live controller; the RECT is passed by value, as the header declares.
        let hr = unsafe { (vt.put_bounds)(self.0.as_ptr(), bounds) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    pub fn webview(&self) -> Result<WebView, HRESULT> {
        let vt: &ControllerVtbl = self.0.vtbl();
        let mut out: *mut c_void = null_mut();
        // SAFETY: `out` is a valid out-pointer for an interface pointer.
        let hr = unsafe { (vt.get_core_webview2)(self.0.as_ptr(), &mut out) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success the callee returned a new reference that we now own.
        unsafe { ComPtr::from_owned(out) }
            .map(WebView)
            .ok_or(E_POINTER)
    }

    /// Closes the controller. The webview stops rendering.
    pub fn close(&self) -> Result<(), HRESULT> {
        let vt: &ControllerVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live controller.
        let hr = unsafe { (vt.close)(self.0.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }
}

impl WebView {
    /// The webview's settings (`get_Settings`). They apply to pages shown from now on.
    pub fn settings(&self) -> Result<Settings, HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let mut out: *mut c_void = null_mut();
        // SAFETY: `out` is a valid out-pointer for an interface pointer.
        let hr = unsafe { (vt.get_settings)(self.0.as_ptr(), &mut out) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success the callee returned a new reference that we now own.
        unsafe { ComPtr::from_owned(out) }
            .map(Settings)
            .ok_or(E_POINTER)
    }

    /// Runs `script` in every document before any page script, from now on. `on_done` receives
    /// the registration result on the UI thread.
    /// Runs `script` in the page. `on_done` receives the call's HRESULT on the UI thread. The
    /// script's result is not read: the completion handler's string argument is ignored.
    pub fn execute_script(
        &self,
        script: &str,
        on_done: impl FnOnce(HRESULT) + 'static,
    ) -> Result<(), HRESULT> {
        let handler =
            new_completed_handler(IID_EXECUTE_SCRIPT_HANDLER, move |hr, _result| on_done(hr));
        let text = wide(script);
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `text` is NUL-terminated and lives across the call; `handler` is live for the call.
        let hr = unsafe { (vt.execute_script)(self.0.as_ptr(), text.as_ptr(), handler) };
        // SAFETY: we release our reference. The webview keeps its own while it needs the callback.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    pub fn add_document_start_script(
        &self,
        script: &str,
        on_done: impl FnOnce(HRESULT) + 'static,
    ) -> Result<(), HRESULT> {
        let handler = new_completed_handler(IID_SCRIPT_ADDED_HANDLER, move |hr, _id| on_done(hr));
        let text = wide(script);
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `text` is NUL-terminated and lives across the call; `handler` is live for the call.
        let hr = unsafe {
            (vt.add_script_to_execute_on_document_created)(self.0.as_ptr(), text.as_ptr(), handler)
        };
        // SAFETY: we release our reference. The webview keeps its own while it needs the callback.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Calls `f` when a WebView2 process fails. Use `WebView::reload` from inside `f` to recover a
    /// page whose renderer exited (spec 10.1, page crash recovery).
    pub fn on_process_failed(
        &self,
        mut f: impl FnMut(&WebView, ProcessFailure) + 'static,
    ) -> Result<(), HRESULT> {
        let handler = new_event_handler(IID_PROCESS_FAILED_HANDLER, move |sender, args| {
            // SAFETY: both pointers are valid for the callback; the wrappers take their own references.
            let (Some(sender), Some(args)) = (unsafe { ComPtr::from_borrowed(sender) }, unsafe {
                ComPtr::from_borrowed(args)
            }) else {
                return;
            };
            let vt: &ProcessFailedArgsVtbl = args.vtbl();
            let mut kind: i32 = -1;
            // SAFETY: `kind` is a valid out-pointer for an enum value.
            let hr = unsafe { (vt.get_process_failed_kind)(args.as_ptr(), &mut kind) };
            if hr >= 0 {
                f(&WebView(sender), ProcessFailure::from_kind(kind));
            }
        });
        let mut token = 0i64;
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
        let hr = unsafe { (vt.add_process_failed)(self.0.as_ptr(), handler, &mut token) };
        // SAFETY: we release our reference. The webview keeps its own.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Reloads the current page.
    pub fn reload(&self) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `self.0` is a live webview.
        let hr = unsafe { (vt.reload)(self.0.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Runs a DevTools protocol method. Crate-private: raw protocol access is not offered to
    /// pages or agents (spec 9, 10.4). Used by tests to crash a renderer on purpose.
    #[cfg(test)]
    pub(crate) fn call_devtools_protocol(
        &self,
        method: &str,
        params_json: &str,
        on_done: impl FnOnce(HRESULT) + 'static,
    ) -> Result<(), HRESULT> {
        let handler =
            new_completed_handler(IID_DEVTOOLS_DONE_HANDLER, move |hr, _result| on_done(hr));
        let name = wide(method);
        let params = wide(params_json);
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: both strings are NUL-terminated and live across the call; `handler` is live.
        let hr = unsafe {
            (vt.call_devtools_protocol_method)(
                self.0.as_ptr(),
                name.as_ptr(),
                params.as_ptr(),
                handler,
            )
        };
        // SAFETY: we release our reference. The webview keeps its own while it needs the callback.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Sends a string to the page. The page sees it in `window.chrome.webview` `message` events.
    pub fn post_web_message(&self, message: &str) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let text = wide(message);
        // SAFETY: `text` is NUL-terminated and lives across the call.
        let hr = unsafe { (vt.post_web_message_as_string)(self.0.as_ptr(), text.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Calls `f` for each string the page posts with `window.chrome.webview.postMessage`.
    /// The `&WebView` passed in is the sender.
    pub fn on_web_message(
        &self,
        mut f: impl FnMut(&WebView, &str) + 'static,
    ) -> Result<(), HRESULT> {
        let handler = new_event_handler(IID_WEB_MESSAGE_RECEIVED_HANDLER, move |sender, args| {
            // SAFETY: both pointers are valid for the duration of this callback. Taking a
            // reference keeps them alive until the wrappers drop.
            let (Some(sender), Some(args)) = (unsafe { ComPtr::from_borrowed(sender) }, unsafe {
                ComPtr::from_borrowed(args)
            }) else {
                return;
            };
            let vt: &WebMessageArgsVtbl = args.vtbl();
            let mut out: *mut u16 = null_mut();
            // SAFETY: `out` is a valid out-pointer.
            let hr = unsafe { (vt.try_get_web_message_as_string)(args.as_ptr(), &mut out) };
            if hr < 0 {
                return;
            }
            // SAFETY: on success `out` is a COM-allocated, NUL-terminated string.
            let text = unsafe { crate::com::take_co_string(out) };
            f(&WebView(sender), &text);
        });
        let mut token = 0i64;
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
        let hr = unsafe { (vt.add_web_message_received)(self.0.as_ptr(), handler, &mut token) };
        // SAFETY: we release our reference. The webview keeps its own.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Adds `uri` to the set of requests passed to [`WebView::on_web_resource_requested`].
    /// Context is ALL (0), so every kind of resource matches.
    pub fn add_web_resource_requested_filter(&self, uri: &str) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let text = wide(uri);
        // SAFETY: `text` is NUL-terminated and lives across the call.
        let hr =
            unsafe { (vt.add_web_resource_requested_filter)(self.0.as_ptr(), text.as_ptr(), 0) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Calls `f` for each request that matches a filter. Answer it with `WebResourceRequest::respond`.
    pub fn on_web_resource_requested(
        &self,
        mut f: impl FnMut(&WebResourceRequest) + 'static,
    ) -> Result<(), HRESULT> {
        let handler =
            new_event_handler(IID_WEB_RESOURCE_REQUESTED_HANDLER, move |_sender, args| {
                // SAFETY: `args` is valid during the callback; the wrapper takes its own reference.
                if let Some(args) = unsafe { ComPtr::from_borrowed(args) } {
                    f(&WebResourceRequest(args));
                }
            });
        let mut token = 0i64;
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
        let hr = unsafe { (vt.add_web_resource_requested)(self.0.as_ptr(), handler, &mut token) };
        // SAFETY: we release our reference. The webview keeps its own.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    /// Calls `allow` for each top-level navigation before it starts. Returning false cancels it.
    /// A URI that cannot be read is cancelled (fail closed). Each call adds one more policy handler.
    pub fn set_navigation_policy(
        &self,
        allow: impl FnMut(&str) -> bool + 'static,
    ) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        register_navigation_policy(self, allow, |web, handler, token| {
            // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
            unsafe { (vt.add_navigation_starting)(web.0.as_ptr(), handler, token) }
        })
    }

    /// Like [`WebView::set_navigation_policy`], for navigations inside iframes. Each frame
    /// navigation is checked against the same `allow` rule; a blocked one is cancelled.
    pub fn set_frame_navigation_policy(
        &self,
        allow: impl FnMut(&str) -> bool + 'static,
    ) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        register_navigation_policy(self, allow, |web, handler, token| {
            // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
            unsafe { (vt.add_frame_navigation_starting)(web.0.as_ptr(), handler, token) }
        })
    }

    pub fn navigate_to_string(&self, html: &str) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let text = wide(html);
        // SAFETY: `text` is NUL-terminated and lives across the call.
        let hr = unsafe { (vt.navigate_to_string)(self.0.as_ptr(), text.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    pub fn navigate(&self, url: &str) -> Result<(), HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let text = wide(url);
        // SAFETY: `text` is NUL-terminated and lives across the call.
        let hr = unsafe { (vt.navigate)(self.0.as_ptr(), text.as_ptr()) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
    }

    pub fn document_title(&self) -> Result<String, HRESULT> {
        let vt: &WebViewVtbl = self.0.vtbl();
        let mut out: *mut u16 = null_mut();
        // SAFETY: `out` is a valid out-pointer for a COM-allocated string.
        let hr = unsafe { (vt.get_document_title)(self.0.as_ptr(), &mut out) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success `out` is a COM-allocated, NUL-terminated string.
        Ok(unsafe { crate::com::take_co_string(out) })
    }
}

/// Builds one navigation-policy handler and registers it through `add`. The handler reads the
/// URI and cancels the navigation when `allow` returns false, or when the URI cannot be read.
fn register_navigation_policy(
    web: &WebView,
    mut allow: impl FnMut(&str) -> bool + 'static,
    add: impl FnOnce(&WebView, *mut c_void, *mut i64) -> HRESULT,
) -> Result<(), HRESULT> {
    let handler = new_event_handler(IID_NAVIGATION_STARTING_HANDLER, move |_sender, args| {
        // SAFETY: `args` is valid during the callback; the wrapper takes its own reference.
        let Some(args) = (unsafe { ComPtr::from_borrowed(args) }) else {
            return;
        };
        let vt: &NavigationArgsVtbl = args.vtbl();
        let mut out: *mut u16 = null_mut();
        // SAFETY: `out` is a valid out-pointer for a COM-allocated string.
        let hr = unsafe { (vt.get_uri)(args.as_ptr(), &mut out) };
        let permitted = if hr < 0 {
            false
        } else {
            // SAFETY: on success `out` is a COM-allocated, NUL-terminated string.
            let uri = unsafe { crate::com::take_co_string(out) };
            allow(&uri)
        };
        if !permitted {
            // SAFETY: `args` is live for the call. 1 is TRUE.
            unsafe { (vt.put_cancel)(args.as_ptr(), 1) };
        }
    });
    let mut token = 0i64;
    let hr = add(web, handler, &mut token);
    // SAFETY: we release our reference. The webview keeps its own.
    unsafe { release(handler) };
    if hr < 0 {
        Err(hr)
    } else {
        Ok(())
    }
}

/// Checks that each slot we call sits at its header index. A reordered vtable would
/// otherwise call the wrong function at runtime.
#[cfg(test)]
mod header_tests {
    use std::path::PathBuf;

    /// (interface, header index, method name) for every slot we call.
    const USED_SLOTS: [(&str, usize, &str); 30] = [
        (
            "ICoreWebView2Environment",
            0,
            "CreateCoreWebView2Controller",
        ),
        ("ICoreWebView2Environment", 1, "CreateWebResourceResponse"),
        ("ICoreWebView2Controller", 3, "put_Bounds"),
        ("ICoreWebView2Controller", 21, "Close"),
        ("ICoreWebView2Controller", 22, "get_CoreWebView2"),
        ("ICoreWebView2", 0, "get_Settings"),
        ("ICoreWebView2", 2, "Navigate"),
        ("ICoreWebView2Settings", 8, "get_AreDevToolsEnabled"),
        ("ICoreWebView2Settings", 9, "put_AreDevToolsEnabled"),
        (
            "ICoreWebView2Settings",
            11,
            "put_AreDefaultContextMenusEnabled",
        ),
        ("ICoreWebView2", 3, "NavigateToString"),
        ("ICoreWebView2", 4, "add_NavigationStarting"),
        ("ICoreWebView2", 22, "add_ProcessFailed"),
        ("ICoreWebView2", 26, "ExecuteScript"),
        ("ICoreWebView2", 28, "Reload"),
        ("ICoreWebView2", 33, "CallDevToolsProtocolMethod"),
        (
            "ICoreWebView2ProcessFailedEventArgs",
            0,
            "get_ProcessFailedKind",
        ),
        ("ICoreWebView2", 14, "add_FrameNavigationStarting"),
        ("ICoreWebView2", 30, "PostWebMessageAsString"),
        ("ICoreWebView2", 31, "add_WebMessageReceived"),
        ("ICoreWebView2", 45, "get_DocumentTitle"),
        ("ICoreWebView2", 52, "add_WebResourceRequested"),
        ("ICoreWebView2", 54, "AddWebResourceRequestedFilter"),
        (
            "ICoreWebView2WebResourceRequestedEventArgs",
            0,
            "get_Request",
        ),
        (
            "ICoreWebView2WebResourceRequestedEventArgs",
            2,
            "put_Response",
        ),
        ("ICoreWebView2WebResourceRequest", 0, "get_Uri"),
        (
            "ICoreWebView2WebMessageReceivedEventArgs",
            2,
            "TryGetWebMessageAsString",
        ),
        ("ICoreWebView2NavigationStartingEventArgs", 0, "get_Uri"),
        ("ICoreWebView2NavigationStartingEventArgs", 5, "put_Cancel"),
        ("ICoreWebView2", 24, "AddScriptToExecuteOnDocumentCreated"),
    ];

    /// Method names of `iface`, in declaration order, from the header text.
    fn methods(text: &str, iface: &str) -> Option<Vec<String>> {
        let mut from = 0;
        while let Some(rel) = text[from..].find(iface) {
            let at = from + rel;
            from = at + iface.len();
            let before = text[..at].trim_end();
            let after = text[at + iface.len()..].trim_start();
            let is_decl = before.ends_with(')')
                && before
                    .rfind("MIDL_INTERFACE(")
                    .is_some_and(|p| at - p < 120)
                && after.starts_with(':')
                && after[1..].trim_start().starts_with("public IUnknown");
            if !is_decl {
                continue;
            }
            let body_end = text[at..].find("};")? + at;
            let body = &text[at..body_end];
            let mut names = Vec::new();
            let mut rest = body;
            while let Some(p) = rest.find("STDMETHODCALLTYPE ") {
                let tail = &rest[p + "STDMETHODCALLTYPE ".len()..];
                let name: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                names.push(name);
                rest = tail;
            }
            return Some(names);
        }
        None
    }

    #[test]
    fn used_slots_match_webview2_header() {
        let Some(dir) = std::env::var_os("TPT_WEBVIEW2_INCLUDE") else {
            eprintln!("skipped: set TPT_WEBVIEW2_INCLUDE to the NuGet build/native/include folder");
            return;
        };
        let path = PathBuf::from(dir).join("WebView2.h");
        let text = std::fs::read_to_string(&path).expect("WebView2.h should be readable");
        for (iface, index, name) in USED_SLOTS {
            let list =
                methods(&text, iface).unwrap_or_else(|| panic!("{iface} not found in header"));
            assert_eq!(
                list.get(index).map(String::as_str),
                Some(name),
                "{iface} slot {index} should be {name}"
            );
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn default_user_data_dir_follows_rfc_default() {
        let dir = default_user_data_dir(Path::new(r"C:\Users\me\AppData\Local"), "my-app").unwrap();
        assert_eq!(
            dir,
            Path::new(r"C:\Users\me\AppData\Local\tpt-shell\my-app\webview2")
        );
    }

    #[test]
    fn default_user_data_dir_rejects_path_tricks() {
        let base = Path::new(r"C:\base");
        for bad in ["", ".", "..", "a/b", r"a\b", "../x", "a b", "C:evil"] {
            assert!(
                default_user_data_dir(base, bad).is_err(),
                "accepted `{bad}`"
            );
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;
    use crate::loader::OfficialLoader;
    use crate::shim::SHIM_JS;
    use crate::window::{pump_until, runtime_test_loader, HiddenWindow};
    use tpt_shell_ipc::{Limits, Router};

    // Pages load their script from app.js: the page policy blocks inline scripts.
    const INDEX_HTML: &[u8] =
        b"<!doctype html><title>loading</title><script src=\"app.js\"></script>";
    const INDEX_JS: &[u8] = b"window.chrome.webview.addEventListener('message', e => { document.title = 'got-' + e.data; });window.chrome.webview.postMessage('ping');";

    /// Serves app content from `APP_ORIGIN` through WebResourceRequested and round-trips a message: the
    /// page posts "ping", Rust replies "pong", and the page sets its title to "got-pong".
    #[test]
    fn serves_virtual_host_and_round_trips_a_message() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-app-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.add_web_resource_requested_filter(format!("{APP_ORIGIN}/*").as_str())
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let (status, content_type, body): (i32, &str, &[u8]) =
                if uri == format!("{APP_ORIGIN}/index.html") {
                    (200, "text/html", INDEX_HTML)
                } else if uri == format!("{APP_ORIGIN}/app.js") {
                    (200, "text/javascript", INDEX_JS)
                } else {
                    (404, "text/plain", b"not found")
                };
            let _ = req.respond(&env, status, content_type, body);
        })
        .expect("resource handler");
        web.on_web_message(|sender, text| {
            if text == "ping" {
                let _ = sender.post_web_message("pong");
            }
        })
        .expect("message handler");
        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");

        let mut title = String::new();
        let got = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "got-pong"
        });
        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        assert!(
            got,
            "page title never became got-pong; last title was {title:?}"
        );
    }

    /// Navigations outside the app origin are cancelled before they start, and the page stays put.
    #[test]
    fn blocks_navigation_outside_the_app_origin() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-nav-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let (status, content_type, body): (i32, &str, &[u8]) =
                if uri == format!("{APP_ORIGIN}/index.html") {
                    (200, "text/html", INDEX_HTML)
                } else if uri == format!("{APP_ORIGIN}/app.js") {
                    (200, "text/javascript", INDEX_JS)
                } else {
                    (404, "text/plain", b"not found")
                };
            let _ = req.respond(&env, status, content_type, body);
        })
        .expect("resource handler");

        let blocked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let blocked_in = blocked.clone();
        web.set_navigation_policy(move |uri| {
            let allowed = uri.starts_with(APP_ORIGIN) || uri == "about:blank";
            if !allowed {
                blocked_in.borrow_mut().push(uri.to_string());
            }
            allowed
        })
        .expect("navigation policy");

        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate to app");
        let mut title = String::new();
        let loaded = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "loading"
        });
        assert!(loaded, "app page did not load; last title {title:?}");

        web.navigate("https://example.com/").expect("navigate out");
        web.navigate("file:///C:/Windows/win.ini")
            .expect("navigate to file");
        let both = pump_until(Duration::from_secs(30), || blocked.borrow().len() >= 2);
        // Give any navigation that was not cancelled time to replace the page.
        pump_until(Duration::from_millis(500), || false);
        let title_after = web.document_title().unwrap_or_default();

        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };

        let urls = blocked.borrow().clone();
        assert!(
            both,
            "navigations were not both blocked; blocked list: {urls:?}"
        );
        assert!(
            urls.iter().any(|u| u.starts_with("https://example.com")),
            "example.com not blocked: {urls:?}"
        );
        assert!(
            urls.iter().any(|u| u.starts_with("file:")),
            "file URL not blocked: {urls:?}"
        );
        assert_eq!(
            title_after, "loading",
            "the page changed after a blocked navigation"
        );
    }

    const INVOKE_HTML: &[u8] =
        b"<!doctype html><title>loading</title><script src=\"app.js\"></script>";
    const INVOKE_JS: &[u8] = b"window.tpt.invoke('echo', { msg: 'hi' }).then(function (d) {window.tpt.invoke('nope').then(null, function (e) { document.title = 'echo:' + d.msg + '|' + e.code; });});";

    /// The page calls `window.tpt.invoke` from the shim: a Rust command answers with data, and an
    /// unknown command answers with a typed error. Both come back through the message channel.
    #[test]
    fn shim_invokes_a_rust_command_and_receives_a_typed_error() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-shim-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        let shim_hr: Rc<RefCell<Option<HRESULT>>> = Rc::new(RefCell::new(None));
        let slot = shim_hr.clone();
        web.add_document_start_script(SHIM_JS, move |hr| *slot.borrow_mut() = Some(hr))
            .expect("start script");
        assert!(
            pump_until(Duration::from_secs(30), || shim_hr.borrow().is_some()),
            "script registration timed out"
        );
        assert!(
            shim_hr.borrow().is_some_and(|hr| hr >= 0),
            "shim registration failed: {:?}",
            shim_hr.borrow()
        );

        let mut router = Router::new();
        router.register("echo", |args| Ok(args.clone()));
        web.on_web_message(move |sender, text| {
            let reply = router.handle(text.as_bytes(), &Limits::default());
            let _ = sender.post_web_message(&reply);
        })
        .expect("message handler");

        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let (status, content_type, body): (i32, &str, &[u8]) =
                if uri == format!("{APP_ORIGIN}/index.html") {
                    (200, "text/html", INVOKE_HTML)
                } else if uri == format!("{APP_ORIGIN}/app.js") {
                    (200, "text/javascript", INVOKE_JS)
                } else {
                    (404, "text/plain", b"not found")
                };
            let _ = req.respond(&env, status, content_type, body);
        })
        .expect("resource handler");
        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");

        let mut title = String::new();
        let got = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "echo:hi|IPC_UNKNOWN_COMMAND"
        });
        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        assert!(
            got,
            "page title was {title:?}, expected echo:hi|IPC_UNKNOWN_COMMAND"
        );
    }

    /// Assets compiled into the test binary from `fixtures/app`.
    static EMBEDDED: &[(&str, &str, &[u8])] = tpt_shell_macros::embed_dir!("../../fixtures/app");

    /// Content type and bytes for an embedded path such as "index.html".
    fn embedded(path: &str) -> Option<(&'static str, &'static [u8])> {
        EMBEDDED
            .iter()
            .find(|(p, _, _)| *p == path)
            .map(|(_, content_type, body)| (*content_type, *body))
    }

    /// Serves embedded files from the app origin, including subresources. The page's script,
    /// `app.js`, sets the title, so the title proves the embedded file was fetched and run.
    #[test]
    fn serves_embedded_assets_from_the_app_origin() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        assert!(
            embedded("index.html").is_some(),
            "embed_dir! found no index.html"
        );

        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-embed-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let path = uri
                .strip_prefix(APP_ORIGIN)
                .unwrap_or("")
                .trim_start_matches('/');
            let path = if path.is_empty() { "index.html" } else { path };
            match embedded(path) {
                Some((content_type, body)) => {
                    let _ = req.respond(&env, 200, content_type, body);
                }
                None => {
                    let _ = req.respond(&env, 404, "text/plain", b"not found");
                }
            }
        })
        .expect("resource handler");
        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");

        let mut title = String::new();
        let got = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "embedded-ok"
        });
        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        assert!(got, "page title was {title:?}, expected embedded-ok");
    }

    /// Process handle count, for leak checks.
    fn process_handles() -> u32 {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let mut n = 0u32;
        // SAFETY: GetCurrentProcess returns a pseudo-handle; `n` is a valid out-pointer.
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut n) };
        n
    }

    /// Creates and closes controllers repeatedly. Live callback objects and process handles must
    /// return to their starting counts. This checks that the COM references balance (spec R3).
    #[test]
    fn controller_cycles_do_not_leak() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-leak-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let cycle = |env: &Environment| {
            let slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
                Rc::new(RefCell::new(None));
            let s = slot.clone();
            env.create_controller(window.hwnd() as isize, move |r| *s.borrow_mut() = Some(r))
                .expect("start controller");
            assert!(
                pump_until(Duration::from_secs(30), || slot.borrow().is_some()),
                "controller timed out"
            );
            let ctrl = slot
                .borrow_mut()
                .take()
                .unwrap()
                .expect("controller creation failed");
            ctrl.close().expect("close controller");
        };
        let settle = || {
            pump_until(Duration::from_millis(500), || false);
        };

        cycle(&env); // warm-up: first use allocates caches the runtime keeps
        settle();
        let handles_before = process_handles();
        let live_before = crate::handler::live();

        for _ in 0..15 {
            cycle(&env);
            settle();
        }
        settle();
        let handles_after = process_handles();
        let live_after = crate::handler::live();

        drop(env);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        let _ = std::fs::remove_dir_all(&data_dir);

        eprintln!("process handles {handles_before} -> {handles_after}; live callbacks {live_before} -> {live_after}");
        assert_eq!(
            live_after, live_before,
            "callback objects leaked across controller cycles"
        );
        assert!(
            handles_after <= handles_before + 10,
            "process handles grew from {handles_before} to {handles_after} over 20 cycles"
        );
    }

    const FRAME_HTML: &[u8] =
        b"<!doctype html><title>loading</title><iframe src=\"https://example.com/\"></iframe>";

    /// An iframe pointing outside the app origin is cancelled. The top-level app page still loads.
    #[test]
    fn blocks_frame_navigation_outside_the_app_origin() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-frame-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let (status, content_type, body): (i32, &str, &[u8]) =
                if uri == format!("{APP_ORIGIN}/index.html") {
                    (200, "text/html", FRAME_HTML)
                } else {
                    (404, "text/plain", b"not found")
                };
            let _ = req.respond(&env, status, content_type, body);
        })
        .expect("resource handler");

        let blocked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let top_blocked = blocked.clone();
        web.set_navigation_policy(move |uri| {
            let allowed = uri.starts_with(APP_ORIGIN) || uri == "about:blank";
            if !allowed {
                top_blocked.borrow_mut().push(format!("top {uri}"));
            }
            allowed
        })
        .expect("top-level policy");
        let frame_blocked = blocked.clone();
        web.set_frame_navigation_policy(move |uri| {
            let allowed = uri.starts_with(APP_ORIGIN) || uri == "about:blank";
            if !allowed {
                frame_blocked.borrow_mut().push(format!("frame {uri}"));
            }
            allowed
        })
        .expect("frame policy");

        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");
        let mut title = String::new();
        let loaded = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "loading"
        });
        let got_frame_block = pump_until(Duration::from_secs(30), || {
            blocked
                .borrow()
                .iter()
                .any(|b| b.starts_with("frame https://example.com"))
        });
        pump_until(Duration::from_millis(500), || false);
        let title_after = web.document_title().unwrap_or_default();

        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };

        let list = blocked.borrow().clone();
        assert!(loaded, "app page did not load; last title {title:?}");
        assert!(
            got_frame_block,
            "iframe to example.com was not blocked; blocked list: {list:?}"
        );
        assert!(
            !list.iter().any(|b| b.starts_with("top ")),
            "the top-level app page was blocked: {list:?}"
        );
        assert_eq!(
            title_after, "loading",
            "the page changed after a blocked frame navigation"
        );
    }

    /// Crashes the renderer on purpose (DevTools `Page.crash`). The shell reports a RenderExited
    /// failure, the callback reloads the page, and the page loads again.
    #[test]
    fn renderer_crash_is_reported_and_the_page_reloads() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-crash-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        let page_loads = Rc::new(Cell::new(0u32));
        let loads = page_loads.clone();
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let path = uri
                .strip_prefix(APP_ORIGIN)
                .unwrap_or("")
                .trim_start_matches('/');
            let path = if path.is_empty() { "index.html" } else { path };
            if path == "index.html" {
                loads.set(loads.get() + 1);
            }
            match embedded(path) {
                Some((content_type, body)) => {
                    let _ = req.respond(&env, 200, content_type, body);
                }
                None => {
                    let _ = req.respond(&env, 404, "text/plain", b"not found");
                }
            }
        })
        .expect("resource handler");

        let failures: Rc<RefCell<Vec<ProcessFailure>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = failures.clone();
        web.on_process_failed(move |sender, failure| {
            recorded.borrow_mut().push(failure);
            if failure == ProcessFailure::RenderExited {
                let _ = sender.reload();
            }
        })
        .expect("process-failed handler");

        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");
        let mut title = String::new();
        let loaded = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "embedded-ok"
        });
        assert!(
            loaded,
            "app page did not load before the crash; last title {title:?}"
        );

        let crash_hr: Rc<RefCell<Option<HRESULT>>> = Rc::new(RefCell::new(None));
        let slot = crash_hr.clone();
        web.call_devtools_protocol("Page.crash", "{}", move |hr| *slot.borrow_mut() = Some(hr))
            .expect("start Page.crash");
        let crashed = pump_until(Duration::from_secs(30), || {
            failures.borrow().contains(&ProcessFailure::RenderExited)
        });
        let reloaded = pump_until(Duration::from_secs(30), || {
            page_loads.get() >= 2 && {
                title = web.document_title().unwrap_or_default();
                title == "embedded-ok"
            }
        });
        let list = failures.borrow().clone();

        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        let _ = std::fs::remove_dir_all(&data_dir);

        assert!(
            crashed,
            "no RenderExited failure reported; failures: {list:?}"
        );
        assert!(
            reloaded,
            "the page did not reload after the crash; loads {}, title {title:?}",
            page_loads.get()
        );
    }

    /// Renders HTML in a hidden window and reads the document title back. Needs a loader
    /// (`TPT_WEBVIEW2_LOADER`) and an installed WebView2 runtime. Skipped otherwise.
    /// Served page for the CSP test: an inline script, then the app's own script.
    const CSP_PAGE: &str = "<!doctype html><title>start</title>\
        <script>document.title = 'inline-ran';</script>\
        <script src=\"app.js\"></script>";

    #[test]
    fn page_csp_blocks_inline_script_and_allows_the_app_script() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let window = HiddenWindow::new().expect("hidden window");
        let (env, ctrl, web) = runtime_webview(&loader_path, &window);
        web.add_web_resource_requested_filter(&format!("{APP_ORIGIN}/*"))
            .expect("filter");
        web.on_web_resource_requested(move |req| {
            let uri = req.uri().unwrap_or_default();
            let path = uri
                .strip_prefix(APP_ORIGIN)
                .unwrap_or("")
                .trim_start_matches('/');
            let (content_type, body): (&str, &[u8]) = match path {
                "" | "index.html" => ("text/html", CSP_PAGE.as_bytes()),
                "app.js" => (
                    "text/javascript",
                    b"document.title = document.title === 'inline-ran' ? 'both' : 'external-only';",
                ),
                _ => {
                    let _ = req.respond(&env, 404, "text/plain", b"not found");
                    return;
                }
            };
            let _ = req.respond(&env, 200, content_type, body);
        })
        .expect("resource handler");
        web.navigate(&format!("{APP_ORIGIN}/index.html"))
            .expect("navigate");

        let mut title = String::new();
        let done = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "external-only" || title == "both"
        });
        ctrl.close().expect("close controller");
        // SAFETY: balances the CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };

        assert!(done, "the app script never ran; title is {title:?}");
        assert_eq!(title, "external-only", "the inline script must be blocked");
    }

    #[test]
    fn every_response_carries_the_page_policy() {
        let headers = response_headers("text/html");
        assert!(headers.starts_with("Content-Type: text/html\r\n"));
        assert!(headers.contains(&format!("Content-Security-Policy: {PAGE_CSP}\r\n")));
        assert!(PAGE_CSP.contains("script-src 'self'"));
        assert!(!PAGE_CSP.contains("unsafe-inline"));
    }

    /// Creates a controller on a hidden window and returns it with its webview.
    fn runtime_webview(
        loader_path: &std::ffi::OsStr,
        window: &HiddenWindow,
    ) -> (Environment, Controller, WebView) {
        let loader = OfficialLoader::load(Path::new(loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let data_dir =
            std::env::temp_dir().join(format!("tpt-shell-settings-{}", std::process::id()));
        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(pump_until(Duration::from_secs(30), || env_slot
            .borrow()
            .is_some()));
        let env = env_slot.borrow_mut().take().unwrap().expect("environment");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(pump_until(Duration::from_secs(30), || ctrl_slot
            .borrow()
            .is_some()));
        let ctrl = ctrl_slot.borrow_mut().take().unwrap().expect("controller");
        let web = ctrl.webview().expect("webview");
        (env, ctrl, web)
    }

    #[test]
    fn devtools_and_context_menu_settings_round_trip() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let window = HiddenWindow::new().expect("hidden window");
        let (_env, ctrl, web) = runtime_webview(&loader_path, &window);
        let settings = web.settings().expect("get_Settings");

        settings.set_devtools_enabled(false).expect("put devtools");
        assert!(!settings.devtools_enabled().expect("get devtools"));
        settings.set_devtools_enabled(true).expect("put devtools");
        assert!(settings.devtools_enabled().expect("get devtools"));
        settings
            .set_default_context_menus_enabled(false)
            .expect("put context menus");

        ctrl.close().expect("close controller");
        // SAFETY: balances the CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
    }

    #[test]
    fn renders_html_and_reads_title() {
        let Some(loader_path) = runtime_test_loader() else {
            return;
        };
        // SAFETY: standard COM init for this thread, balanced by CoUninitialize below.
        let co = unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        assert!(co >= 0, "CoInitializeEx failed: 0x{co:08X}");

        let loader = OfficialLoader::load(Path::new(&loader_path)).expect("loader should load");
        let create = loader
            .create_environment_fn()
            .expect("environment entry point");
        let window = HiddenWindow::new().expect("hidden window");
        let data_dir = std::env::temp_dir().join(format!("tpt-shell-spike-{}", std::process::id()));

        let env_slot: Rc<RefCell<Option<Result<Environment, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = env_slot.clone();
        create_environment(create, &data_dir, move |r| *slot.borrow_mut() = Some(r))
            .expect("start environment");
        assert!(
            pump_until(Duration::from_secs(30), || env_slot.borrow().is_some()),
            "environment timed out"
        );
        let env = env_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("environment creation failed");

        let ctrl_slot: Rc<RefCell<Option<Result<Controller, HRESULT>>>> =
            Rc::new(RefCell::new(None));
        let slot = ctrl_slot.clone();
        env.create_controller(window.hwnd() as isize, move |r| {
            *slot.borrow_mut() = Some(r)
        })
        .expect("start controller");
        assert!(
            pump_until(Duration::from_secs(30), || ctrl_slot.borrow().is_some()),
            "controller timed out"
        );
        let ctrl = ctrl_slot
            .borrow_mut()
            .take()
            .unwrap()
            .expect("controller creation failed");

        let web = ctrl.webview().expect("webview");
        web.navigate_to_string("<!doctype html><title>tpt-shell-spike</title><p>hello</p>")
            .expect("navigate_to_string");
        let mut title = String::new();
        let got = pump_until(Duration::from_secs(30), || {
            title = web.document_title().unwrap_or_default();
            title == "tpt-shell-spike"
        });
        ctrl.close().expect("close controller");
        drop(web);
        drop(ctrl);
        drop(env);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        let _ = std::fs::remove_dir_all(&data_dir);
        assert!(
            got,
            "document title never became tpt-shell-spike; last title was {title:?}"
        );
    }
}
