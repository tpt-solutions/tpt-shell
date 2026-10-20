// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! WebView2 environment, controller and webview, through hand-written vtables (RFC 0001).
//!
//! Each vtable declares only the slots we call. Slots before them are placeholders sized
//! from the header, so slot positions match `WebView2.h`. The header test below checks that.

use std::ffi::c_void;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;

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
pub const IID_NAVIGATION_STARTING_HANDLER: Guid = Guid {
    data1: 0x9adbe429,
    data2: 0xf36d,
    data3: 0x432b,
    data4: [0x9d, 0xdc, 0xf8, 0x88, 0x1f, 0xbd, 0x76, 0xe3],
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
    _before_close: [usize; 21],
    close: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_core_webview2: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct WebViewVtbl {
    base: IUnknownVtbl,
    _before_navigate: [usize; 2],
    navigate: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    navigate_to_string: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    add_navigation_starting:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_script: [usize; 19],
    add_script_to_execute_on_document_created:
        unsafe extern "system" fn(*mut c_void, *const u16, *mut c_void) -> HRESULT,
    _before_post_message: [usize; 5],
    post_web_message_as_string: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    add_web_message_received:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut i64) -> HRESULT,
    _before_document_title: [usize; 13],
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
        let headers = wide(&format!("Content-Type: {content_type}\r\n"));
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

/// `ICoreWebView2Environment`.
pub struct Environment(ComPtr);

/// `ICoreWebView2Controller`.
pub struct Controller(ComPtr);

/// `ICoreWebView2`.
pub struct WebView(ComPtr);

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

impl Controller {
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
    /// Runs `script` in every document before any page script, from now on. `on_done` receives
    /// the registration result on the UI thread.
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
    /// A URI that cannot be read is cancelled (fail closed). Frame navigations are not covered yet.
    /// Each call adds one more policy handler; the policies all run.
    pub fn set_navigation_policy(
        &self,
        mut allow: impl FnMut(&str) -> bool + 'static,
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
        let vt: &WebViewVtbl = self.0.vtbl();
        // SAFETY: `handler` is live for the call; `token` is a valid out-pointer.
        let hr = unsafe { (vt.add_navigation_starting)(self.0.as_ptr(), handler, &mut token) };
        // SAFETY: we release our reference. The webview keeps its own.
        unsafe { release(handler) };
        if hr < 0 {
            Err(hr)
        } else {
            Ok(())
        }
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

/// Checks that each slot we call sits at its header index. A reordered vtable would
/// otherwise call the wrong function at runtime.
#[cfg(test)]
mod header_tests {
    use std::path::PathBuf;

    /// (interface, header index, method name) for every slot we call.
    const USED_SLOTS: [(&str, usize, &str); 19] = [
        (
            "ICoreWebView2Environment",
            0,
            "CreateCoreWebView2Controller",
        ),
        ("ICoreWebView2Environment", 1, "CreateWebResourceResponse"),
        ("ICoreWebView2Controller", 21, "Close"),
        ("ICoreWebView2Controller", 22, "get_CoreWebView2"),
        ("ICoreWebView2", 2, "Navigate"),
        ("ICoreWebView2", 3, "NavigateToString"),
        ("ICoreWebView2", 4, "add_NavigationStarting"),
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
mod runtime_tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;
    use crate::loader::OfficialLoader;
    use crate::shim::SHIM_JS;
    use crate::window::{pump_until, HiddenWindow};
    use tpt_shell_ipc::{Limits, Router};

    const INDEX_HTML: &[u8] = b"<!doctype html><title>loading</title><script>window.chrome.webview.addEventListener('message', e => { document.title = 'got-' + e.data; });window.chrome.webview.postMessage('ping');</script>";

    /// Serves app content from `APP_ORIGIN` through WebResourceRequested and round-trips a message: the
    /// page posts "ping", Rust replies "pong", and the page sets its title to "got-pong".
    #[test]
    fn serves_virtual_host_and_round_trips_a_message() {
        let Some(loader_path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
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
        let Some(loader_path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
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

    const INVOKE_HTML: &[u8] = b"<!doctype html><title>loading</title><script>window.tpt.invoke('echo', { msg: 'hi' }).then(function (d) {window.tpt.invoke('nope').then(null, function (e) { document.title = 'echo:' + d.msg + '|' + e.code; });});</script>";

    /// The page calls `window.tpt.invoke` from the shim: a Rust command answers with data, and an
    /// unknown command answers with a typed error. Both come back through the message channel.
    #[test]
    fn shim_invokes_a_rust_command_and_receives_a_typed_error() {
        let Some(loader_path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
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
        let Some(loader_path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
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

    /// Renders HTML in a hidden window and reads the document title back. Needs a loader
    /// (`TPT_WEBVIEW2_LOADER`) and an installed WebView2 runtime. Skipped otherwise.
    #[test]
    fn renders_html_and_reads_title() {
        let Some(loader_path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
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
