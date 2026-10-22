// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The GTK 3 and WebKit2GTK 4.1 adapter (todo M5, first adapter; GTK 4 follows).
//!
//! Status: written against the GTK 3 and WebKit2GTK 4.1 C APIs, and checked here for the Linux
//! target only (no link, no run). It has NOT been run on a Linux machine. Its behaviour is
//! untested until the CI Linux job (headless Xvfb) runs the adapter tests.
//!
//! Support by feature:
//! - Full: title, size, fullscreen, close action, page messages (`window.chrome.webview`),
//!   `eval`, `app://` assets (scheme handler), navigation policy, clipboard text.
//! - Partial: frameless (no drag regions yet, so a frameless window cannot be moved by its
//!   background); always-on-top and bounds (a Wayland compositor may ignore them);
//!   transparency (needs a compositing window manager and must be set before the window is
//!   shown); snapshot (the visible region, PNG); a web-process crash reloads the page, up to
//!   five times.
//! - Unsupported here: drag regions, backdrops, child and modal windows.

/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub mod dialogs;
/// Support: Full.
pub mod route;
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub mod sys;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::time::{Duration, Instant};

use tpt_shell_core::backend::{
    AssetSource, CloseAction, EventLoopBackend, EventLoopProxy, MessageHandler, WebviewBackend,
    WindowBackend, WindowSpec,
};
use tpt_shell_core::platform::{ClipboardBackend, ColorScheme, ThemeBackend};
use tpt_shell_core::{Error, LogicalRect, LogicalSize, Result};

use sys::{Gtk3, Obj, Rgba, GTK_WINDOW_TOPLEVEL};

/// How many web-process crashes a window survives by reloading before it stops reloading.
const MAX_CRASH_RELOADS: u32 = 5;

/// How long a snapshot may take before it is abandoned.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);

/// The key under which a web view stores a pointer to its window state.
const VIEW_KEY: &CStr = c"tpt-shell-window";

/// Handle to a window managed by [`Gtk3Backend`].
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WindowId(pub usize);

/// Everything the callbacks need about one window. Boxed, so its address is stable while the
/// window exists. It is freed when the window is closed, or is found destroyed.
pub(crate) struct WindowState {
    pub(crate) gtk: &'static Gtk3,
    pub(crate) window: Obj,
    pub(crate) view: Obj,
    pub(crate) destroyed: Cell<bool>,
    pub(crate) shown: Cell<bool>,
    pub(crate) close_action: Cell<CloseAction>,
    pub(crate) allow: Vec<String>,
    pub(crate) handler: RefCell<Option<MessageHandler>>,
    pub(crate) assets: RefCell<Option<AssetSource>>,
    pub(crate) crashes: Cell<u32>,
}

/// Owns the GTK windows. Create it on the thread that will run the event loop.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Gtk3Backend {
    pub(crate) gtk: &'static Gtk3,
    next: usize,
    pub(crate) windows: HashMap<usize, Box<WindowState>>,
    scheme_registered: bool,
    /// Windows that were closed. Their state is leaked on purpose: WebKit may still call a signal
    /// handler or the scheme handler with the state pointer after the widget is destroyed, so
    /// the memory must stay valid. Handlers and assets are dropped; `destroyed` is set.
    #[allow(clippy::vec_box)] // the boxed address is what WebKit callbacks hold
    retired: Vec<Box<WindowState>>,
}

/// The `Unsupported` error for a feature this adapter does not provide.
fn unsupported(feature: &'static str) -> Error {
    Error::Unsupported { feature }
}

/// A C string from Rust text. Text with a NUL byte is refused.
pub(crate) fn cstring(text: &str) -> Result<CString> {
    CString::new(text).map_err(|_| Error::Backend("text holds a NUL byte".into()))
}

/// Copies a C string into a Rust string. A null pointer gives an empty string.
///
/// # Safety
/// `ptr` is null or a valid NUL-terminated string.
pub(crate) unsafe fn from_c(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

impl Gtk3Backend {
    /// Starts GTK on the current thread. Fails when no display is available or GTK cannot open
    /// one. The error names the library or display that was missing.
    ///
    /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn new() -> Result<Gtk3Backend> {
        let gtk = sys::load().map_err(|e| Error::Backend(e.to_string()))?;
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        if !has_display {
            return Err(Error::Backend(
                "no display: neither DISPLAY nor WAYLAND_DISPLAY is set".into(),
            ));
        }
        // SAFETY: null argc and argv are allowed by gtk_init_check.
        let ok = unsafe { (gtk.fns.gtk_init_check)(std::ptr::null_mut(), std::ptr::null_mut()) };
        if ok == 0 {
            return Err(Error::Backend("GTK could not open the display".into()));
        }
        Ok(Gtk3Backend {
            gtk,
            next: 1,
            windows: HashMap::new(),
            scheme_registered: false,
            retired: Vec::new(),
        })
    }

    /// Runs the GTK main loop until [`EventLoopBackend::quit`] is called.
    ///
    /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn run(&mut self) {
        // SAFETY: called on the thread that started GTK.
        unsafe { (self.gtk.fns.gtk_main)() };
        drain_default_context(self.gtk);
    }

    /// Connects a signal. `callback` must have the signature GTK documents for the signal, and
    /// `data` must stay valid until the object is destroyed.
    fn connect(
        &self,
        obj: Obj,
        signal: &str,
        callback: *const c_void,
        data: *mut c_void,
    ) -> Result<()> {
        let name = cstring(signal)?;
        // SAFETY: `obj` is a live GObject, and the callers pass callbacks with the signatures
        // the signals use. The window state, which `data` points to, is freed only after the
        // object is destroyed.
        unsafe {
            (self.gtk.fns.g_signal_connect_data)(
                obj,
                name.as_ptr(),
                callback,
                data,
                std::ptr::null(),
                0,
            )
        };
        Ok(())
    }

    /// Connects the window's signals and installs the page bridge. Called once, by `create`.
    fn wire(&self, state: &WindowState) -> Result<()> {
        let f = &self.gtk.fns;
        let (window, view) = (state.window, state.view);
        let data = (state as *const WindowState as *mut WindowState).cast::<c_void>();
        self.connect(window, "destroy", on_destroy as *const c_void, data)?;
        self.connect(window, "delete-event", on_delete as *const c_void, data)?;
        self.connect(
            view,
            "decide-policy",
            on_decide_policy as *const c_void,
            data,
        )?;
        self.connect(
            view,
            "web-process-crashed",
            on_crashed as *const c_void,
            data,
        )?;

        // SAFETY: the view is live; the manager belongs to it.
        let manager = unsafe { (f.webkit_web_view_get_user_content_manager)(view) };
        let handler_name = cstring(route::MESSAGE_HANDLER)?;
        // SAFETY: the manager and name are valid for the call.
        let registered = unsafe {
            (f.webkit_user_content_manager_register_script_message_handler)(
                manager,
                handler_name.as_ptr(),
            )
        };
        if registered == 0 {
            return Err(Error::Backend(
                "WebKit refused the message handler name".into(),
            ));
        }
        self.connect(
            manager,
            "script-message-received::tptShell",
            on_script_message as *const c_void,
            data,
        )?;
        let polyfill = cstring(route::PAGE_POLYFILL)?;
        // SAFETY: a user script is created, handed to the manager (which keeps a reference),
        // then released by us.
        unsafe {
            let script = (f.webkit_user_script_new)(
                polyfill.as_ptr(),
                sys::WEBKIT_INJECT_TOP_FRAME,
                sys::WEBKIT_INJECT_AT_DOCUMENT_START,
                std::ptr::null(),
                std::ptr::null(),
            );
            if !script.is_null() {
                (f.webkit_user_content_manager_add_script)(manager, script);
                (f.webkit_user_script_unref)(script);
            }
        }
        Ok(())
    }

    /// Drops the entries for windows the user closed directly. Their widgets are gone.
    /// Keeps a closed window's state alive for good (see the `retired` field).
    fn retire(&mut self, w: Box<WindowState>) {
        w.destroyed.set(true);
        *w.handler.borrow_mut() = None;
        *w.assets.borrow_mut() = None;
        self.retired.push(w);
    }

    fn prune(&mut self) {
        let gone: Vec<usize> = self
            .windows
            .iter()
            .filter(|(_, w)| w.destroyed.get())
            .map(|(id, _)| *id)
            .collect();
        let any = !gone.is_empty();
        for id in gone {
            if let Some(w) = self.windows.remove(&id) {
                self.retire(w);
            }
        }
        if any {
            drain_default_context(self.gtk);
        }
    }

    fn state(&self, id: WindowId) -> Result<&WindowState> {
        self.windows
            .get(&id.0)
            .map(|b| &**b)
            .filter(|w| !w.destroyed.get())
            .ok_or(Error::UnknownWindow(id.0 as u64))
    }

    /// Current position and size, in logical units.
    ///
    /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn bounds(&self, id: WindowId) -> Result<LogicalRect> {
        let w = self.state(id)?;
        let (mut x, mut y, mut width, mut height) = (0, 0, 0, 0);
        // SAFETY: the window is live (checked above) and the out pointers are valid.
        unsafe {
            (self.gtk.fns.gtk_window_get_position)(w.window, &mut x, &mut y);
            (self.gtk.fns.gtk_window_get_size)(w.window, &mut width, &mut height);
        }
        Ok(LogicalRect {
            x: f64::from(x),
            y: f64::from(y),
            width: f64::from(width),
            height: f64::from(height),
        })
    }

    /// Moves and resizes the window. A Wayland compositor may decide the position itself.
    ///
    /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn set_bounds(&mut self, id: WindowId, rect: LogicalRect) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe {
            (self.gtk.fns.gtk_window_move)(w.window, rect.x as c_int, rect.y as c_int);
            (self.gtk.fns.gtk_window_resize)(w.window, rect.width as c_int, rect.height as c_int);
        }
        Ok(())
    }

    /// Captures the visible region of the page as PNG. Blocks the UI thread (it runs a nested
    /// main loop) for up to [`SNAPSHOT_TIMEOUT`].
    ///
    /// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn snapshot_png(&mut self, id: WindowId) -> Result<Vec<u8>> {
        let view = self.state(id)?.view;
        let f = &self.gtk.fns;
        let mut wait = SnapshotWait {
            gtk: self.gtk,
            finished: false,
            surface: std::ptr::null_mut(),
        };
        // SAFETY: the view is live; `wait` outlives the callback, which runs before this
        // function returns because the loop below waits for it.
        unsafe {
            (f.webkit_web_view_get_snapshot)(
                view,
                sys::WEBKIT_SNAPSHOT_REGION_VISIBLE,
                sys::WEBKIT_SNAPSHOT_OPTIONS_NONE,
                std::ptr::null_mut(),
                snapshot_ready as *const c_void,
                (&mut wait as *mut SnapshotWait).cast(),
            )
        };
        let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
        loop {
            if wait.finished {
                break;
            }
            if Instant::now() > deadline {
                return Err(Error::Backend("the snapshot did not finish in time".into()));
            }
            // SAFETY: blocking iteration of the main loop on the UI thread.
            unsafe { (f.gtk_main_iteration_do)(1) };
        }
        if wait.surface.is_null() {
            return Err(Error::Backend("the page could not be captured".into()));
        }
        let mut png = Vec::new();
        // SAFETY: the surface is valid until destroyed below; `png` outlives the call.
        let status = unsafe {
            (f.cairo_surface_write_to_png_stream)(
                wait.surface,
                write_png_bytes as *const c_void,
                (&mut png as *mut Vec<u8>).cast(),
            )
        };
        // SAFETY: the surface came from WebKit and is not used after this.
        unsafe { (f.cairo_surface_destroy)(wait.surface) };
        if status != sys::CAIRO_STATUS_SUCCESS {
            return Err(Error::Backend(
                "the snapshot could not be encoded as PNG".into(),
            ));
        }
        Ok(png)
    }
}

/// Runs the default main context until it has been idle for three polls in a row (10 ms apart),
/// or half a second has passed. Disposing a web view is asynchronous: WebKit finishes closing the
/// page and ending its web process from the main context. A process that exits before that
/// happens can hit a WebKit assertion in an `atexit` handler, so every path that destroys a
/// window drains afterwards.
pub(crate) fn drain_default_context(gtk: &Gtk3) {
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut idle = 0;
    while Instant::now() < deadline {
        // SAFETY: GTK is initialised and this runs on its thread.
        let pending = unsafe { (gtk.fns.gtk_events_pending)() };
        if pending != 0 {
            idle = 0;
            // SAFETY: a non-blocking iteration of the default context.
            unsafe { (gtk.fns.gtk_main_iteration_do)(0) };
            continue;
        }
        idle += 1;
        if idle >= 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// State shared with the snapshot callback while the loop waits.
struct SnapshotWait {
    gtk: &'static Gtk3,
    finished: bool,
    surface: Obj,
}

/// GAsyncReadyCallback for `webkit_web_view_get_snapshot`.
///
/// # Safety
/// Called by GTK with the web view, the async result, and the `SnapshotWait` passed in.
unsafe extern "C" fn snapshot_ready(source: Obj, result: Obj, data: *mut c_void) {
    // SAFETY: `data` is the `SnapshotWait` the waiting loop owns, alive until it finishes.
    let wait = unsafe { &mut *data.cast::<SnapshotWait>() };
    let f = &wait.gtk.fns;
    let mut error: Obj = std::ptr::null_mut();
    // SAFETY: the arguments are the ones WebKit passed to this callback.
    unsafe {
        wait.surface = (f.webkit_web_view_get_snapshot_finish)(source, result, &mut error);
        if !error.is_null() {
            (f.g_error_free)(error);
        }
    }
    wait.finished = true;
}

/// cairo_write_func_t: appends PNG bytes to the `Vec` the closure points to.
///
/// # Safety
/// `closure` is the `Vec<u8>` passed to `cairo_surface_write_to_png_stream`, and `data` holds
/// `length` readable bytes, as cairo guarantees.
unsafe extern "C" fn write_png_bytes(closure: *mut c_void, data: *const u8, length: u32) -> c_int {
    // SAFETY: see the function's contract above.
    unsafe {
        let out = &mut *closure.cast::<Vec<u8>>();
        out.extend_from_slice(std::slice::from_raw_parts(data, length as usize));
    }
    sys::CAIRO_STATUS_SUCCESS
}

// ---------------------------------------------------------------------------------------------
// GTK signal callbacks. Each receives the window state pointer as its last argument.
// ---------------------------------------------------------------------------------------------

/// `destroy` on the window: mark it gone; the backend frees the state at its next call.
///
/// # Safety
/// `data` is the live `WindowState` the window was created with.
unsafe extern "C" fn on_destroy(_window: Obj, data: *mut c_void) {
    // SAFETY: see the contract above.
    unsafe { &*data.cast::<WindowState>() }.destroyed.set(true);
}

/// `delete-event`: the title bar close button. Hides the window when the close action is Hide.
/// Returns TRUE (1) to stop GTK destroying it, FALSE (0) to let it close.
///
/// # Safety
/// `data` is the live `WindowState`.
unsafe extern "C" fn on_delete(_window: Obj, _event: Obj, data: *mut c_void) -> c_int {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    match w.close_action.get() {
        CloseAction::Close => 0,
        CloseAction::Hide => {
            // SAFETY: the window is live while its signal runs.
            unsafe { (w.gtk.fns.gtk_widget_hide)(w.window) };
            1
        }
    }
}

/// `decide-policy` on the web view. Navigations are allowed by [`route::navigation_allowed`];
/// new windows are refused.
///
/// # Safety
/// `decision` is the WebKit decision object for this request; `data` is the live state.
unsafe extern "C" fn on_decide_policy(
    _view: Obj,
    decision: Obj,
    kind: c_int,
    data: *mut c_void,
) -> c_int {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    let f = &w.gtk.fns;
    let allowed = if kind == sys::WEBKIT_POLICY_DECISION_NAVIGATION_ACTION {
        // SAFETY: a navigation decision has a navigation action and request for its lifetime.
        let uri = unsafe {
            let action = (f.webkit_navigation_policy_decision_get_navigation_action)(decision);
            let request = (f.webkit_navigation_action_get_request)(action);
            from_c((f.webkit_uri_request_get_uri)(request))
        };
        route::navigation_allowed(&uri, &w.allow)
    } else {
        // New windows are refused. Responses pass: the navigation that produced them was
        // checked above.
        kind != sys::WEBKIT_POLICY_DECISION_NEW_WINDOW_ACTION
    };
    // SAFETY: each decision is answered exactly once.
    unsafe {
        if allowed {
            (f.webkit_policy_decision_use)(decision);
        } else {
            (f.webkit_policy_decision_ignore)(decision);
        }
    }
    1
}

/// `web-process-crashed`: reload the page, up to a limit, so a crashed renderer does not leave a
/// blank window with no way back.
///
/// # Safety
/// `view` is the live web view; `data` is the live state.
unsafe extern "C" fn on_crashed(view: Obj, data: *mut c_void) -> c_int {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    if w.destroyed.get() {
        return 1;
    }
    let n = w.crashes.get() + 1;
    w.crashes.set(n);
    if n <= MAX_CRASH_RELOADS {
        // SAFETY: the view is live.
        unsafe { (w.gtk.fns.webkit_web_view_reload)(view) };
    } else {
        eprintln!("tpt-shell: the web process crashed {n} times; not reloading");
    }
    1
}

/// `script-message-received::tptShell`: the page posted a string. The handler's reply, if any, is
/// delivered back to the page as a message event.
///
/// # Safety
/// `result` is the WebKit message for this call; `data` is the live state.
unsafe extern "C" fn on_script_message(_manager: Obj, result: Obj, data: *mut c_void) {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    if w.destroyed.get() {
        return;
    }
    let f = &w.gtk.fns;
    // SAFETY: WebKit passes a result object that is valid during this signal.
    let text = unsafe {
        let value = (f.webkit_javascript_result_get_js_value)(result);
        if value.is_null() {
            return;
        }
        let raw = (f.jsc_value_to_string)(value);
        let text = from_c(raw);
        (f.g_free)(raw.cast());
        text
    };
    // Take the handler out while it runs, so it may replace itself without a borrow conflict.
    let taken = w.handler.borrow_mut().take();
    let Some(mut handler) = taken else {
        return;
    };
    let reply = handler(&text);
    if w.handler.borrow().is_none() {
        *w.handler.borrow_mut() = Some(handler);
    }
    if let Some(reply) = reply {
        if let Ok(script) = cstring(&route::dispatch_script(&reply)) {
            // SAFETY: the view is live; the script is a valid C string for the call.
            unsafe {
                (f.webkit_web_view_run_javascript)(
                    w.view,
                    script.as_ptr(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                )
            };
        }
    }
}

/// The `app` URI scheme. Each request is answered from the window's asset source, or with a
/// not-found error. The blob route reaches the source with the path `__tpt/blob/<id>`.
///
/// # Safety
/// `request` is the WebKit request for this call. `data` is the `&'static Gtk3` the scheme was
/// registered with.
unsafe extern "C" fn on_app_scheme(request: Obj, data: *mut c_void) {
    // SAFETY: `data` is the static adapter registered with the scheme (see the contract).
    let gtk: &'static Gtk3 = unsafe { &*data.cast::<Gtk3>() };
    let f = &gtk.fns;
    // SAFETY: the request is valid during this callback.
    let (uri, view) = unsafe {
        (
            from_c((f.webkit_uri_scheme_request_get_uri)(request)),
            (f.webkit_uri_scheme_request_get_web_view)(request),
        )
    };
    // SAFETY: the view is the one this scheme was registered on; the pointer it carries is the
    // window state, which lives while the view does.
    let state = unsafe { (f.g_object_get_data)(view, VIEW_KEY.as_ptr()) }.cast::<WindowState>();
    let asset = if state.is_null() {
        None
    } else {
        // SAFETY: see above.
        let w = unsafe { &*state };
        route::asset_key(&uri)
            .and_then(|key| w.assets.borrow().as_ref().and_then(|source| source(&key)))
    };
    // SAFETY: the request is answered exactly once below.
    unsafe {
        match asset {
            Some(asset) => {
                let len = asset.body.len();
                // The stream owns a copy that GLib frees.
                let buffer = (f.g_malloc)(len.max(1));
                std::ptr::copy_nonoverlapping(asset.body.as_ptr(), buffer.cast::<u8>(), len);
                let stream =
                    (f.g_memory_input_stream_new_from_data)(buffer, len, f.g_free as *const c_void);
                let mime = CString::new(asset.content_type.as_str())
                    .unwrap_or_else(|_| c"application/octet-stream".into());
                (f.webkit_uri_scheme_request_finish)(request, stream, len as i64, mime.as_ptr());
                (f.g_object_unref)(stream);
            }
            None => {
                let domain = (f.g_quark_from_string)(c"tpt-shell".as_ptr());
                let error = (f.g_error_new_literal)(domain, 404, c"not found".as_ptr());
                (f.webkit_uri_scheme_request_finish_error)(request, error);
                (f.g_error_free)(error);
            }
        }
    }
}

impl WindowBackend for Gtk3Backend {
    type Id = WindowId;

    fn create(&mut self, spec: &WindowSpec) -> Result<WindowId> {
        self.prune();
        let f = &self.gtk.fns;
        let title = cstring(&spec.title)?;
        // SAFETY: GTK is initialised (Gtk3Backend::new). GTK owns each widget once it is
        // added to its parent; the state below is boxed and lives as long as the widgets.
        let (window, view) = unsafe {
            let window = (f.gtk_window_new)(GTK_WINDOW_TOPLEVEL);
            if window.is_null() {
                return Err(Error::Backend("GTK could not create a window".into()));
            }
            (f.gtk_window_set_default_size)(
                window,
                spec.size.width as c_int,
                spec.size.height as c_int,
            );
            (f.gtk_window_set_title)(window, title.as_ptr());
            let view = (f.webkit_web_view_new)();
            if view.is_null() {
                (f.gtk_widget_destroy)(window);
                return Err(Error::Backend("WebKit could not create a web view".into()));
            }
            (f.gtk_container_add)(window, view);
            (window, view)
        };
        let state = Box::new(WindowState {
            gtk: self.gtk,
            window,
            view,
            destroyed: Cell::new(false),
            shown: Cell::new(false),
            close_action: Cell::new(CloseAction::Close),
            allow: spec.allow_navigation.clone(),
            handler: RefCell::new(None),
            assets: RefCell::new(None),
            crashes: Cell::new(0),
        });
        if let Err(e) = self.wire(&state) {
            // SAFETY: the window is live, and `state` lives until this function returns, so the
            // destroy handler can still set its flag.
            unsafe { (f.gtk_widget_destroy)(window) };
            return Err(e);
        }
        let id = self.next;
        self.next += 1;
        self.windows.insert(id, state);
        Ok(WindowId(id))
    }

    fn show(&mut self, id: WindowId) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe { (self.gtk.fns.gtk_widget_show_all)(w.window) };
        w.shown.set(true);
        Ok(())
    }

    fn hide(&mut self, id: WindowId) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe { (self.gtk.fns.gtk_widget_hide)(w.window) };
        Ok(())
    }

    fn set_title(&mut self, id: WindowId, title: &str) -> Result<()> {
        let title = cstring(title)?;
        let w = self.state(id)?;
        // SAFETY: the window is live; the title is a valid C string for the call.
        unsafe { (self.gtk.fns.gtk_window_set_title)(w.window, title.as_ptr()) };
        Ok(())
    }

    fn set_size(&mut self, id: WindowId, size: LogicalSize) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe {
            (self.gtk.fns.gtk_window_resize)(w.window, size.width as c_int, size.height as c_int)
        };
        Ok(())
    }

    fn set_fullscreen(&mut self, id: WindowId, on: bool) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe {
            if on {
                (self.gtk.fns.gtk_window_fullscreen)(w.window);
            } else {
                (self.gtk.fns.gtk_window_unfullscreen)(w.window);
            }
        }
        Ok(())
    }

    fn close(&mut self, id: WindowId) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live. Destroying it runs `on_destroy`, which only sets a flag,
        // so the state is still valid when it is removed below.
        unsafe { (self.gtk.fns.gtk_widget_destroy)(w.window) };
        if let Some(w) = self.windows.remove(&id.0) {
            self.retire(w);
        }
        // The destroy only schedules the web view's disposal; let WebKit finish it.
        drain_default_context(self.gtk);
        Ok(())
    }

    fn set_frameless(&mut self, id: WindowId, on: bool) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe { (self.gtk.fns.gtk_window_set_decorated)(w.window, c_int::from(!on)) };
        Ok(())
    }

    fn set_always_on_top(&mut self, id: WindowId, on: bool) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe { (self.gtk.fns.gtk_window_set_keep_above)(w.window, c_int::from(on)) };
        Ok(())
    }

    fn set_transparent(&mut self, id: WindowId, on: bool) -> Result<()> {
        let w = self.state(id)?;
        if w.shown.get() {
            return Err(unsupported(
                "transparency (set it before the window is shown)",
            ));
        }
        let f = &self.gtk.fns;
        // SAFETY: the window and view are live; the screen and visual come from GTK.
        unsafe {
            if on {
                let screen = (f.gtk_widget_get_screen)(w.window);
                if screen.is_null() || (f.gdk_screen_is_composited)(screen) == 0 {
                    return Err(unsupported(
                        "transparency (needs a compositing window manager)",
                    ));
                }
                let visual = (f.gdk_screen_get_rgba_visual)(screen);
                if visual.is_null() {
                    return Err(unsupported("transparency (no RGBA visual on this screen)"));
                }
                (f.gtk_widget_set_visual)(w.window, visual);
                (f.gtk_widget_set_app_paintable)(w.window, 1);
                let clear = Rgba {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 0.0,
                };
                (f.webkit_web_view_set_background_color)(w.view, &clear);
            } else {
                (f.gtk_widget_set_visual)(w.window, std::ptr::null_mut());
                (f.gtk_widget_set_app_paintable)(w.window, 0);
                let white = Rgba {
                    red: 1.0,
                    green: 1.0,
                    blue: 1.0,
                    alpha: 1.0,
                };
                (f.webkit_web_view_set_background_color)(w.view, &white);
            }
        }
        Ok(())
    }

    fn set_close_action(&mut self, id: WindowId, action: CloseAction) -> Result<()> {
        self.state(id)?.close_action.set(action);
        Ok(())
    }
}

impl WebviewBackend for Gtk3Backend {
    fn load_html(&mut self, id: WindowId, html: &str) -> Result<()> {
        let html = cstring(html)?;
        let base = cstring(&format!("{}/", route::APP_ORIGIN))?;
        let w = self.state(id)?;
        // SAFETY: the view is live; both strings are valid for the call.
        unsafe { (self.gtk.fns.webkit_web_view_load_html)(w.view, html.as_ptr(), base.as_ptr()) };
        Ok(())
    }

    fn navigate(&mut self, id: WindowId, url: &str) -> Result<()> {
        if !route::is_app_uri(url) && !route::navigation_allowed(url, &self.state(id)?.allow) {
            return Err(Error::Backend(format!(
                "navigation refused by policy: {url}"
            )));
        }
        let url = cstring(url)?;
        let w = self.state(id)?;
        // SAFETY: the view is live; the URL is a valid C string for the call.
        unsafe { (self.gtk.fns.webkit_web_view_load_uri)(w.view, url.as_ptr()) };
        Ok(())
    }

    fn eval(&mut self, id: WindowId, script: &str) -> Result<()> {
        let script = cstring(script)?;
        let w = self.state(id)?;
        // SAFETY: the view is live; the script is a valid C string for the call.
        unsafe {
            (self.gtk.fns.webkit_web_view_run_javascript)(
                w.view,
                script.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        Ok(())
    }

    fn post_message(&mut self, id: WindowId, message: &str) -> Result<()> {
        let script = cstring(&route::dispatch_script(message))?;
        let w = self.state(id)?;
        // SAFETY: the view is live; the script is a valid C string for the call.
        unsafe {
            (self.gtk.fns.webkit_web_view_run_javascript)(
                w.view,
                script.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        Ok(())
    }

    fn set_message_handler(&mut self, id: WindowId, handler: MessageHandler) -> Result<()> {
        *self.state(id)?.handler.borrow_mut() = Some(handler);
        Ok(())
    }

    fn set_asset_source(&mut self, id: WindowId, source: AssetSource) -> Result<()> {
        let w = self.state(id)?;
        *w.assets.borrow_mut() = Some(source);
        let view = w.view;
        let state_ptr = (w as *const WindowState).cast_mut().cast::<c_void>();
        let f = &self.gtk.fns;
        // SAFETY: the view is live. The pointer stored on it is the boxed state, which lives
        // until the view is destroyed; the scheme callback reads it only while the view exists.
        unsafe { (f.g_object_set_data)(view, VIEW_KEY.as_ptr(), state_ptr) };
        if !self.scheme_registered {
            let scheme = cstring("app")?;
            let gtk_ptr = (self.gtk as *const Gtk3).cast_mut().cast::<c_void>();
            // SAFETY: the context belongs to the view; the callback has the scheme signature,
            // and the adapter it receives is static.
            unsafe {
                let context = (f.webkit_web_view_get_context)(view);
                (f.webkit_web_context_register_uri_scheme)(
                    context,
                    scheme.as_ptr(),
                    on_app_scheme as *const c_void,
                    gtk_ptr,
                    std::ptr::null(),
                );
            }
            self.scheme_registered = true;
        }
        Ok(())
    }
}

/// The GTK main loop as the event loop of the shell.
impl EventLoopBackend for Gtk3Backend {
    type Proxy = Gtk3Proxy;

    fn proxy(&self) -> Gtk3Proxy {
        Gtk3Proxy { gtk: self.gtk }
    }

    fn quit(&mut self) {
        // SAFETY: called on the UI thread, where GTK runs.
        unsafe { (self.gtk.fns.gtk_main_quit)() };
    }
}

impl Drop for Gtk3Backend {
    fn drop(&mut self) {
        for (_, w) in self.windows.drain() {
            if !w.destroyed.get() {
                // SAFETY: the window is live until destroyed here; the state is freed after.
                unsafe { (self.gtk.fns.gtk_widget_destroy)(w.window) };
            }
            // The state stays allocated: WebKit may still call a handler with its pointer.
            Box::leak(w);
        }
        for w in std::mem::take(&mut self.retired) {
            Box::leak(w);
        }
        drain_default_context(self.gtk);
    }
}

/// Cross-thread handle to the GTK main loop. Posting uses `g_idle_add`, which GLib makes safe
/// to call from any thread.
///
/// Support: Partial. Needs GTK 3 and WebKit2GTK 4.1 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[derive(Clone, Copy)]
pub struct Gtk3Proxy {
    gtk: &'static Gtk3,
}

impl EventLoopProxy for Gtk3Proxy {
    fn post(&self, task: Box<dyn FnOnce() + Send>) -> Result<()> {
        let raw = Box::into_raw(Box::new(task)).cast::<c_void>();
        // SAFETY: `run_task` is called once with `raw` on the UI thread, and takes it back.
        let id = unsafe { (self.gtk.fns.g_idle_add)(run_task, raw) };
        if id == 0 {
            // SAFETY: GLib did not take the task, so it is still ours to free.
            drop(unsafe { Box::from_raw(raw.cast::<Box<dyn FnOnce() + Send>>()) });
            return Err(Error::Backend("the main loop refused the task".into()));
        }
        Ok(())
    }
}

/// The idle callback that runs one posted task. Returns FALSE so it runs once.
///
/// # Safety
/// `data` is the pointer `Gtk3Proxy::post` made from a boxed task, and runs exactly once.
unsafe extern "C" fn run_task(data: *mut c_void) -> c_int {
    // SAFETY: see the contract above.
    let task = unsafe { Box::from_raw(data.cast::<Box<dyn FnOnce() + Send>>()) };
    (*task)();
    0
}

impl ClipboardBackend for Gtk3Backend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised. The atom and clipboard are owned by GTK.
        let raw = unsafe {
            let atom = (f.gdk_atom_intern)(c"CLIPBOARD".as_ptr(), 0);
            let clipboard = (f.gtk_clipboard_get)(atom);
            (f.gtk_clipboard_wait_for_text)(clipboard)
        };
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: GTK returned an owned string, freed here.
        let text = unsafe {
            let text = from_c(raw);
            (f.g_free)(raw.cast());
            text
        };
        Ok(Some(text))
    }

    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        let c = cstring(text)?;
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised; the text is copied by GTK during the call.
        unsafe {
            let atom = (f.gdk_atom_intern)(c"CLIPBOARD".as_ptr(), 0);
            let clipboard = (f.gtk_clipboard_get)(atom);
            (f.gtk_clipboard_set_text)(clipboard, c.as_ptr(), text.len() as c_int);
            // Keep the text after this process exits, as the other platforms do.
            (f.gtk_clipboard_store)(clipboard);
        }
        Ok(())
    }
}

impl ThemeBackend for Gtk3Backend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        crate::dbus::settings::color_scheme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when GTK 3 and WebKit 4.1 load and a display variable is set. Otherwise the tests
    /// below skip: they need a display (a desktop session, or Xvfb in CI).
    fn can_run_gtk() -> bool {
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        has_display && sys::load().is_ok()
    }

    #[test]
    fn without_a_display_the_adapter_says_so_and_does_not_crash() {
        if can_run_gtk() {
            return;
        }
        assert!(Gtk3Backend::new().is_err());
    }

    #[test]
    fn a_window_round_trip_under_a_display() {
        if !can_run_gtk() {
            eprintln!("skipped: no display or no GTK 3 and WebKit 4.1 libraries");
            return;
        }
        // GTK 3 and GTK 4 cannot share a process, and GTK runs one test at a time.
        let Some(_gui) = crate::testutil::gui_session("gtk3") else {
            return;
        };
        let mut backend = Gtk3Backend::new().expect("GTK starts when a display is present");
        let spec = WindowSpec {
            name: "main".into(),
            title: "tpt test".into(),
            size: LogicalSize {
                width: 320.0,
                height: 240.0,
            },
            allow_navigation: vec![],
        };
        let id = backend.create(&spec).expect("a window is created");
        backend.set_title(id, "renamed").expect("title");
        backend
            .set_size(
                id,
                LogicalSize {
                    width: 300.0,
                    height: 200.0,
                },
            )
            .expect("size");
        backend.set_always_on_top(id, true).expect("always on top");
        backend.set_frameless(id, true).expect("frameless");
        backend.close(id).expect("close");
        assert!(backend.state(id).is_err(), "a closed window is unknown");
    }

    #[test]
    fn the_route_for_a_page_message_is_quoted_safely() {
        // Pure: runs everywhere. The script never contains the raw message.
        let script = route::dispatch_script("</script>");
        assert!(!script.contains("</script>"));
    }
}
