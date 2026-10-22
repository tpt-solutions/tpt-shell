// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The GTK 4 and WebKitGTK 6.0 adapter (todo M5, second adapter).
//!
//! Status: written against the GTK 4 and WebKitGTK 6.0 C APIs, and checked here for the Linux
//! targets only (no link, no run). It has NOT been run on a Linux machine from this workspace.
//! Its behaviour is untested until the CI Linux adapter jobs (Xvfb, and headless Weston) run
//! the adapter tests.
//!
//! It mirrors [`crate::gtk3`] and shares its pure rules ([`crate::gtk3::route`]) and its dialog
//! description types. What differs, because GTK 4 changed:
//!
//! - The event loop is a plain GLib `GMainLoop` (GTK 4 has no `gtk_main`).
//! - There is no window position API (a Wayland client never had one, and GTK 4 removed it for
//!   X11 too), no keep-above API and no GTK-level `get_position`: [`Gtk4Backend::bounds`],
//!   [`Gtk4Backend::set_bounds`] and always-on-top return `Unsupported`.
//! - A web-process crash arrives as `web-process-terminated` with a reason.
//! - The snapshot is a `GdkTexture`, encoded as PNG by GDK (no Cairo).
//! - The clipboard is a `GdkClipboard`, read asynchronously.
//! - File dialogs use `GtkFileDialog` (GTK 4.10 and later); on an older GTK 4 they return
//!   `Unsupported`.
//!
//! Support by feature:
//! - Full: title, size, fullscreen, close action, page messages (`window.chrome.webview`),
//!   `eval`, `app://` assets (scheme handler), navigation policy.
//! - Partial: frameless (no drag regions yet); transparency (needs a compositing display);
//!   snapshot (the visible region, PNG); clipboard text (a Wayland compositor only hands over
//!   the clipboard to a focused client); a web-process crash reloads the page, up to five times;
//!   file dialogs (GTK 4.10 and later).
//! - Unsupported here: window position and bounds, always-on-top, drag regions, backdrops,
//!   child and modal windows.

/// Support: Partial. Needs GTK 4.10 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub mod dialogs;
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub mod sys;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::rc::Rc;
use std::time::{Duration, Instant};

use tpt_shell_core::backend::{
    AssetSource, CloseAction, EventLoopBackend, EventLoopProxy, MessageHandler, WebviewBackend,
    WindowBackend, WindowSpec,
};
use tpt_shell_core::platform::{ClipboardBackend, ColorScheme, ThemeBackend};
use tpt_shell_core::{Error, LogicalRect, LogicalSize, Result};

use crate::gtk3::route;
use crate::gtk3::{cstring, from_c};
use sys::{Gtk4, Obj, Rgba};

pub use crate::gtk3::WindowId;

/// How many web-process crashes a window survives by reloading before it stops reloading.
const MAX_CRASH_RELOADS: u32 = 5;

/// How long a snapshot may take before it is abandoned.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a clipboard read may take before it is abandoned.
const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(2);

/// The key under which a web view stores a pointer to its window state.
const VIEW_KEY: &CStr = c"tpt-shell-window";

/// The CSS class that makes a window background see-through.
const TRANSPARENT_CLASS: &CStr = c"tpt-transparent";

/// Everything the callbacks need about one window. Boxed, so its address is stable while the
/// window exists. It is freed when the window is closed, or is found destroyed.
pub(crate) struct WindowState {
    pub(crate) gtk: &'static Gtk4,
    pub(crate) window: Obj,
    pub(crate) view: Obj,
    pub(crate) destroyed: Cell<bool>,
    pub(crate) close_action: Cell<CloseAction>,
    pub(crate) allow: Vec<String>,
    pub(crate) handler: RefCell<Option<MessageHandler>>,
    pub(crate) assets: RefCell<Option<AssetSource>>,
    pub(crate) crashes: Cell<u32>,
}

/// Owns the GTK 4 windows. Create it on the thread that will run the event loop.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
pub struct Gtk4Backend {
    pub(crate) gtk: &'static Gtk4,
    next: usize,
    pub(crate) windows: HashMap<usize, Box<WindowState>>,
    scheme_registered: bool,
    /// `TPT_WEBKIT_CONTEXT=own`: use a web context and network session made and released by
    /// this backend, not WebKit's process-wide defaults (an experiment for an exit-time
    /// WebKit abort). The session is ephemeral, so nothing is stored on disk.
    own_context: bool,
    context: Obj,
    session: Obj,
    /// Windows that were closed. Their state is leaked on purpose: WebKit may still call a signal
    /// handler or the scheme handler with the state pointer after the widget is destroyed, so
    /// the memory must stay valid. Handlers and assets are dropped; `destroyed` is set.
    #[allow(clippy::vec_box)] // the boxed address is what WebKit callbacks hold
    retired: Vec<Box<WindowState>>,
    transparency_css: bool,
    main_loop: Obj,
}

/// The `Unsupported` error for a feature this adapter does not provide.
pub(crate) fn unsupported(feature: &'static str) -> Error {
    Error::Unsupported { feature }
}

/// Runs the default main context until `done` is true, or `timeout` passes. `None` waits without
/// a limit. Returns whether `done` became true. Must run on the thread that started GTK.
pub(crate) fn pump(gtk: &Gtk4, mut done: impl FnMut() -> bool, timeout: Option<Duration>) -> bool {
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if done() {
            return true;
        }
        if deadline.is_some_and(|d| Instant::now() > d) {
            return false;
        }
        // SAFETY: a non-blocking iteration of the default context on the UI thread.
        let ran = unsafe { (gtk.fns.g_main_context_iteration)(std::ptr::null_mut(), 0) };
        if ran == 0 {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Runs the default main context until it has been idle for three polls in a row (10 ms apart),
/// or half a second has passed. Disposing a web view is asynchronous: WebKit finishes closing the
/// page and ending its web process from the main context. A process that exits before that
/// happens can hit a WebKit assertion in an `atexit` handler, so every path that destroys a
/// window drains afterwards.
pub(crate) fn drain_default_context(gtk: &Gtk4) {
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut idle = 0;
    while Instant::now() < deadline {
        // SAFETY: a non-blocking iteration of the default context on the UI thread.
        let ran = unsafe { (gtk.fns.g_main_context_iteration)(std::ptr::null_mut(), 0) };
        if ran != 0 {
            idle = 0;
            continue;
        }
        idle += 1;
        if idle >= 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Destroys a window and its web view now. The view is detached first, so its last reference
/// drops here and not when the window is eventually disposed. The adapter holds no other
/// reference to the view: `gtk_window_set_child` took the only one, the user script was
/// released after it was added, and the pointer stored on the view is a plain pointer without a
/// reference.
///
/// # Safety
/// `w.window` is a live window of `w.gtk`.
unsafe fn destroy_window(w: &WindowState) {
    // SAFETY: see the contract above.
    unsafe {
        (w.gtk.fns.gtk_window_set_child)(w.window, std::ptr::null_mut());
        (w.gtk.fns.gtk_window_destroy)(w.window);
    }
}

/// The state of one asynchronous call whose waiter may give up. The callback holds one `Rc` and
/// the waiter another, so a callback that fires after the waiter stopped waiting still has live
/// memory to write to, and frees what it was given instead of leaking it.
pub(crate) struct Pending {
    done: Cell<bool>,
    abandoned: Cell<bool>,
    ptr: Cell<*mut c_void>,
}

impl Pending {
    /// A new pending call and the pointer to pass as callback data. The callback must give the
    /// pointer to [`Pending::finish`] exactly once.
    pub(crate) fn start() -> (Rc<Pending>, *mut c_void) {
        let pending = Rc::new(Pending {
            done: Cell::new(false),
            abandoned: Cell::new(false),
            ptr: Cell::new(std::ptr::null_mut()),
        });
        let raw = Rc::into_raw(Rc::clone(&pending))
            .cast_mut()
            .cast::<c_void>();
        (pending, raw)
    }

    /// Called by the callback: records `value`, or frees it with `free` when the waiter gave up.
    ///
    /// # Safety
    /// `data` came from [`Pending::start`] and is used once.
    pub(crate) unsafe fn finish(
        data: *mut c_void,
        value: *mut c_void,
        free: impl FnOnce(*mut c_void),
    ) {
        // SAFETY: see the contract above; this takes back the `Rc` that `start` leaked.
        let pending = unsafe { Rc::from_raw(data.cast::<Pending>()) };
        if pending.abandoned.get() {
            if !value.is_null() {
                free(value);
            }
        } else {
            pending.ptr.set(value);
        }
        pending.done.set(true);
    }

    /// Waits for the callback, then returns what it recorded (null for nothing). After a
    /// timeout, returns `None` and marks the call abandoned.
    pub(crate) fn wait(&self, gtk: &Gtk4, timeout: Option<Duration>) -> Option<*mut c_void> {
        if pump(gtk, || self.done.get(), timeout) {
            Some(self.ptr.replace(std::ptr::null_mut()))
        } else {
            self.abandoned.set(true);
            None
        }
    }
}

impl Gtk4Backend {
    /// Starts GTK on the current thread. Fails when no display is available or GTK cannot open
    /// one. The error names the library or display that was missing.
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn new() -> Result<Gtk4Backend> {
        let gtk = sys::load().map_err(|e| Error::Backend(e.to_string()))?;
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        if !has_display {
            return Err(Error::Backend(
                "no display: neither DISPLAY nor WAYLAND_DISPLAY is set".into(),
            ));
        }
        // SAFETY: gtk_init_check takes no arguments in GTK 4.
        let ok = unsafe { (gtk.fns.gtk_init_check)() };
        if ok == 0 {
            return Err(Error::Backend("GTK could not open the display".into()));
        }
        // SAFETY: a new loop on the default context; owned by the backend, freed in Drop.
        let main_loop = unsafe { (gtk.fns.g_main_loop_new)(std::ptr::null_mut(), 0) };
        if main_loop.is_null() {
            return Err(Error::Backend("GLib could not create a main loop".into()));
        }
        Ok(Gtk4Backend {
            gtk,
            next: 1,
            windows: HashMap::new(),
            scheme_registered: false,
            own_context: std::env::var("TPT_WEBKIT_CONTEXT").is_ok_and(|v| v == "own"),
            context: std::ptr::null_mut(),
            session: std::ptr::null_mut(),
            retired: Vec::new(),
            transparency_css: false,
            main_loop,
        })
    }

    /// Runs the GLib main loop until [`EventLoopBackend::quit`] is called.
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn run(&mut self) {
        // SAFETY: called on the thread that started GTK; the loop is live.
        unsafe { (self.gtk.fns.g_main_loop_run)(self.main_loop) };
        drain_default_context(self.gtk);
    }

    /// Runs the main loop's pending work until `done` is true or `timeout` passes. Returns
    /// whether `done` became true. For tests and for callers that drive the loop themselves.
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn run_until(&mut self, done: impl FnMut() -> bool, timeout: Duration) -> bool {
        pump(self.gtk, done, Some(timeout))
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
        self.connect(
            window,
            "close-request",
            on_close_request as *const c_void,
            data,
        )?;
        self.connect(
            view,
            "decide-policy",
            on_decide_policy as *const c_void,
            data,
        )?;
        self.connect(
            view,
            "web-process-terminated",
            on_terminated as *const c_void,
            data,
        )?;

        // SAFETY: the view is live; the manager belongs to it.
        let manager = unsafe { (f.webkit_web_view_get_user_content_manager)(view) };
        let handler_name = cstring(route::MESSAGE_HANDLER)?;
        // SAFETY: the manager and name are valid for the call; a null world is the default one.
        let registered = unsafe {
            (f.webkit_user_content_manager_register_script_message_handler)(
                manager,
                handler_name.as_ptr(),
                std::ptr::null(),
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

    pub(crate) fn state(&self, id: WindowId) -> Result<&WindowState> {
        self.windows
            .get(&id.0)
            .map(|b| &**b)
            .filter(|w| !w.destroyed.get())
            .ok_or(Error::UnknownWindow(id.0 as u64))
    }

    /// Runs a script in a page and ignores its result.
    fn run_script(&self, view: Obj, script: &str) -> Result<()> {
        let script = cstring(script)?;
        // SAFETY: the view is live; the script is a valid C string for the call (WebKit copies
        // it); a length of -1 means NUL-terminated; the other pointers are optional.
        unsafe {
            (self.gtk.fns.webkit_web_view_evaluate_javascript)(
                view,
                script.as_ptr(),
                -1,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
            )
        };
        Ok(())
    }

    /// The window's current size, in logical units. GTK 4 reports 0 until the window has been
    /// shown and laid out, which is an error here rather than a made-up size.
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn size(&self, id: WindowId) -> Result<LogicalSize> {
        let w = self.state(id)?;
        // SAFETY: the window is live (checked above).
        let (width, height) = unsafe {
            (
                (self.gtk.fns.gtk_widget_get_width)(w.window),
                (self.gtk.fns.gtk_widget_get_height)(w.window),
            )
        };
        if width <= 0 || height <= 0 {
            return Err(Error::Backend(
                "the window has no size until it is shown".into(),
            ));
        }
        Ok(LogicalSize {
            width: f64::from(width),
            height: f64::from(height),
        })
    }

    /// GTK 4 has no window position, so there are no bounds to read. Use [`Gtk4Backend::size`].
    ///
    /// Support: Unsupported. GTK 4 has no window position API.
    pub fn bounds(&self, id: WindowId) -> Result<LogicalRect> {
        self.state(id)?;
        Err(unsupported("window position (GTK 4 has no position API)"))
    }

    /// GTK 4 cannot place a window, so this refuses and changes nothing. Use
    /// [`WindowBackend::set_size`] for the size.
    ///
    /// Support: Unsupported. GTK 4 has no window position API.
    pub fn set_bounds(&mut self, id: WindowId, _rect: LogicalRect) -> Result<()> {
        self.state(id)?;
        Err(unsupported("window position (GTK 4 has no position API)"))
    }

    /// Captures the visible region of the page as PNG. Runs the main loop (so the UI thread keeps
    /// painting) for up to [`SNAPSHOT_TIMEOUT`].
    ///
    /// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn snapshot_png(&mut self, id: WindowId) -> Result<Vec<u8>> {
        let view = self.state(id)?.view;
        let f = &self.gtk.fns;
        let (pending, data) = Pending::start();
        // SAFETY: the view is live; `data` is a leaked `Rc` that `snapshot_ready` takes back
        // exactly once, whether or not we are still waiting.
        unsafe {
            (f.webkit_web_view_get_snapshot)(
                view,
                sys::WEBKIT_SNAPSHOT_REGION_VISIBLE,
                sys::WEBKIT_SNAPSHOT_OPTIONS_NONE,
                std::ptr::null_mut(),
                snapshot_ready as *const c_void,
                data,
            )
        };
        let Some(texture) = pending.wait(self.gtk, Some(SNAPSHOT_TIMEOUT)) else {
            return Err(Error::Backend("the snapshot did not finish in time".into()));
        };
        if texture.is_null() {
            return Err(Error::Backend("the page could not be captured".into()));
        }
        // SAFETY: the texture is valid until unreferenced below. The bytes object is owned by
        // us; its data stays valid until it is unreferenced, and is copied out first.
        let png = unsafe {
            let bytes = (f.gdk_texture_save_to_png_bytes)(texture);
            let png = if bytes.is_null() {
                None
            } else {
                let mut len = 0usize;
                let ptr = (f.g_bytes_get_data)(bytes, &mut len);
                let copy = if ptr.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(ptr.cast::<u8>(), len).to_vec()
                };
                (f.g_bytes_unref)(bytes);
                Some(copy)
            };
            (f.g_object_unref)(texture);
            png
        };
        match png {
            Some(png) if !png.is_empty() => Ok(png),
            _ => Err(Error::Backend(
                "the snapshot could not be encoded as PNG".into(),
            )),
        }
    }

    /// The default display's clipboard.
    fn clipboard(&self) -> Result<Obj> {
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised; the display and clipboard are owned by GTK.
        let clipboard = unsafe {
            let display = (f.gdk_display_get_default)();
            if display.is_null() {
                return Err(Error::Backend("GTK has no default display".into()));
            }
            (f.gdk_display_get_clipboard)(display)
        };
        if clipboard.is_null() {
            return Err(Error::Backend("GDK has no clipboard".into()));
        }
        Ok(clipboard)
    }
}

/// GAsyncReadyCallback for `webkit_web_view_get_snapshot`.
///
/// # Safety
/// Called by GIO with the web view, the async result, and the pointer from `Pending::start`.
unsafe extern "C" fn snapshot_ready(source: Obj, result: Obj, data: *mut c_void) {
    // Unreachable failure: this callback only exists after a successful load. Leak rather than
    // guess at a free function.
    let Ok(gtk) = sys::load() else {
        return;
    };
    let f = &gtk.fns;
    let mut error: Obj = std::ptr::null_mut();
    // SAFETY: the arguments are the ones WebKit passed to this callback; `data` is the pending
    // call, used once.
    unsafe {
        let texture = (f.webkit_web_view_get_snapshot_finish)(source, result, &mut error);
        if !error.is_null() {
            (f.g_error_free)(error);
        }
        Pending::finish(data, texture, |t| (f.g_object_unref)(t));
    }
}

/// GAsyncReadyCallback for `gdk_clipboard_read_text_async`.
///
/// # Safety
/// Called by GIO with the clipboard, the async result, and the pointer from `Pending::start`.
unsafe extern "C" fn clipboard_read_ready(source: Obj, result: Obj, data: *mut c_void) {
    let Ok(gtk) = sys::load() else {
        return;
    };
    let f = &gtk.fns;
    let mut error: Obj = std::ptr::null_mut();
    // SAFETY: the arguments are the ones GDK passed to this callback; `data` is the pending
    // call, used once.
    unsafe {
        let text = (f.gdk_clipboard_read_text_finish)(source, result, &mut error);
        if !error.is_null() {
            (f.g_error_free)(error);
        }
        Pending::finish(data, text.cast(), |t| (f.g_free)(t));
    }
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

/// `close-request`: the title bar close button. Hides the window when the close action is Hide.
/// Returns TRUE (1) to stop GTK destroying it, FALSE (0) to let it close.
///
/// # Safety
/// `data` is the live `WindowState`.
unsafe extern "C" fn on_close_request(_window: Obj, data: *mut c_void) -> c_int {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    match w.close_action.get() {
        CloseAction::Close => 0,
        CloseAction::Hide => {
            // SAFETY: the window is live while its signal runs.
            unsafe { (w.gtk.fns.gtk_widget_set_visible)(w.window, 0) };
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

/// `web-process-terminated`: reload the page, up to a limit, so a crashed renderer does not leave
/// a blank window with no way back. A termination the app asked for is left alone.
///
/// # Safety
/// `view` is the live web view; `data` is the live state.
unsafe extern "C" fn on_terminated(view: Obj, reason: c_int, data: *mut c_void) {
    if reason == sys::WEBKIT_WEB_PROCESS_TERMINATED_BY_API {
        return;
    }
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    if w.destroyed.get() {
        return;
    }
    let n = w.crashes.get() + 1;
    w.crashes.set(n);
    if n <= MAX_CRASH_RELOADS {
        // SAFETY: the view is live.
        unsafe { (w.gtk.fns.webkit_web_view_reload)(view) };
    } else {
        eprintln!("tpt-shell: the web process ended {n} times; not reloading");
    }
}

/// `script-message-received::tptShell`: the page posted a string. The handler's reply, if any, is
/// delivered back to the page as a message event.
///
/// # Safety
/// `value` is the `JSCValue` of the message, valid during this signal; `data` is the live state.
unsafe extern "C" fn on_script_message(_manager: Obj, value: Obj, data: *mut c_void) {
    // SAFETY: see the contract above.
    let w = unsafe { &*data.cast::<WindowState>() };
    if w.destroyed.get() {
        return;
    }
    let f = &w.gtk.fns;
    if value.is_null() {
        return;
    }
    // SAFETY: `value` is valid during this signal; the string JSC returns is ours to free.
    let text = unsafe {
        let raw = (f.jsc_value_to_string)(value);
        let text = from_c(raw);
        if !raw.is_null() {
            (f.g_free)(raw.cast());
        }
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
                (f.webkit_web_view_evaluate_javascript)(
                    w.view,
                    script.as_ptr(),
                    -1,
                    std::ptr::null(),
                    std::ptr::null(),
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
/// `request` is the WebKit request for this call. `data` is the `&'static Gtk4` the scheme was
/// registered with.
unsafe extern "C" fn on_app_scheme(request: Obj, data: *mut c_void) {
    // SAFETY: `data` is the static adapter registered with the scheme (see the contract).
    let gtk: &'static Gtk4 = unsafe { &*data.cast::<Gtk4>() };
    let f = &gtk.fns;
    // SAFETY: the request is valid during this callback.
    let (uri, view) = unsafe {
        (
            from_c((f.webkit_uri_scheme_request_get_uri)(request)),
            (f.webkit_uri_scheme_request_get_web_view)(request),
        )
    };
    // SAFETY: the view is the one this scheme was registered on; the pointer it carries is the
    // window state, which lives while the view does. A request with no view (a worker, say)
    // finds no state and is answered as not found.
    let state = if view.is_null() {
        std::ptr::null_mut()
    } else {
        unsafe { (f.g_object_get_data)(view, VIEW_KEY.as_ptr()) }.cast::<WindowState>()
    };
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
                let mime = std::ffi::CString::new(asset.content_type.as_str())
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

impl WindowBackend for Gtk4Backend {
    type Id = WindowId;

    fn create(&mut self, spec: &WindowSpec) -> Result<WindowId> {
        self.prune();
        let f = &self.gtk.fns;
        let title = cstring(&spec.title)?;
        // SAFETY: GTK is initialised (Gtk4Backend::new). The window owns its child once it is
        // set; the state below is boxed and lives as long as the widgets.
        let (window, view) = unsafe {
            let window = (f.gtk_window_new)();
            if window.is_null() {
                return Err(Error::Backend("GTK could not create a window".into()));
            }
            (f.gtk_window_set_default_size)(
                window,
                spec.size.width as c_int,
                spec.size.height as c_int,
            );
            (f.gtk_window_set_title)(window, title.as_ptr());
            let view = if self.own_context {
                if self.context.is_null() {
                    self.context = (f.webkit_web_context_new)();
                    self.session = (f.webkit_network_session_new_ephemeral)();
                }
                // The view's two construct-only properties, then the NULL that ends the list.
                (self.gtk.g_object_new)(
                    (f.webkit_web_view_get_type)(),
                    c"web-context".as_ptr(),
                    self.context,
                    c"network-session".as_ptr(),
                    self.session,
                    std::ptr::null::<c_char>(),
                )
            } else {
                (f.webkit_web_view_new)()
            };
            if view.is_null() {
                (f.gtk_window_destroy)(window);
                return Err(Error::Backend("WebKit could not create a web view".into()));
            }
            (f.gtk_window_set_child)(window, view);
            (window, view)
        };
        let state = Box::new(WindowState {
            gtk: self.gtk,
            window,
            view,
            destroyed: Cell::new(false),
            close_action: Cell::new(CloseAction::Close),
            allow: spec.allow_navigation.clone(),
            handler: RefCell::new(None),
            assets: RefCell::new(None),
            crashes: Cell::new(0),
        });
        if let Err(e) = self.wire(&state) {
            // SAFETY: the window is live, and `state` lives until this function returns, so the
            // destroy handler can still set its flag.
            unsafe { (f.gtk_window_destroy)(window) };
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
        unsafe { (self.gtk.fns.gtk_window_present)(w.window) };
        Ok(())
    }

    fn hide(&mut self, id: WindowId) -> Result<()> {
        let w = self.state(id)?;
        // SAFETY: the window is live.
        unsafe { (self.gtk.fns.gtk_widget_set_visible)(w.window, 0) };
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
        // SAFETY: the window is live. In GTK 4, setting the default size of a shown window
        // resizes it (gtk_window_resize was removed).
        unsafe {
            (self.gtk.fns.gtk_window_set_default_size)(
                w.window,
                size.width as c_int,
                size.height as c_int,
            )
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
        unsafe { destroy_window(w) };
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

    // `set_always_on_top` keeps the trait default (Unsupported): GTK 4 has no keep-above call,
    // and Wayland has no client-side way to ask for it.

    fn set_transparent(&mut self, id: WindowId, on: bool) -> Result<()> {
        self.state(id)?;
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised; the display, provider and widgets are live, and GTK keeps
        // the provider for the life of the display.
        unsafe {
            let display = (f.gdk_display_get_default)();
            if on && (display.is_null() || (f.gdk_display_is_composited)(display) == 0) {
                return Err(unsupported(
                    "transparency (needs a compositing display; none is running)",
                ));
            }
            if display.is_null() {
                return Err(Error::Backend("GTK has no default display".into()));
            }
            if !self.transparency_css {
                let provider = (f.gtk_css_provider_new)();
                let css = c"window.tpt-transparent { background-color: transparent; }";
                (f.gtk_css_provider_load_from_data)(provider, css.as_ptr(), -1);
                (f.gtk_style_context_add_provider_for_display)(
                    display,
                    provider,
                    sys::GTK_STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                self.transparency_css = true;
            }
        }
        let w = self.state(id)?;
        // SAFETY: the window and view are live.
        unsafe {
            if on {
                (f.gtk_widget_add_css_class)(w.window, TRANSPARENT_CLASS.as_ptr());
                let clear = Rgba {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 0.0,
                };
                (f.webkit_web_view_set_background_color)(w.view, &clear);
            } else {
                (f.gtk_widget_remove_css_class)(w.window, TRANSPARENT_CLASS.as_ptr());
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

impl WebviewBackend for Gtk4Backend {
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
        let view = self.state(id)?.view;
        self.run_script(view, script)
    }

    fn post_message(&mut self, id: WindowId, message: &str) -> Result<()> {
        let view = self.state(id)?.view;
        self.run_script(view, &route::dispatch_script(message))
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
            let gtk_ptr = (self.gtk as *const Gtk4).cast_mut().cast::<c_void>();
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

    fn snapshot(&mut self, id: WindowId) -> Result<Vec<u8>> {
        self.snapshot_png(id)
    }
}

/// The GLib main loop as the event loop of the shell.
impl EventLoopBackend for Gtk4Backend {
    type Proxy = Gtk4Proxy;

    fn proxy(&self) -> Gtk4Proxy {
        Gtk4Proxy { gtk: self.gtk }
    }

    fn quit(&mut self) {
        // SAFETY: called on the UI thread; the loop is live.
        unsafe { (self.gtk.fns.g_main_loop_quit)(self.main_loop) };
    }
}

impl Drop for Gtk4Backend {
    fn drop(&mut self) {
        for (_, w) in self.windows.drain() {
            if !w.destroyed.get() {
                // SAFETY: the window is live until destroyed here; the state is freed after.
                unsafe { destroy_window(&w) };
            }
            // The state stays allocated: WebKit may still call a handler with its pointer.
            Box::leak(w);
        }
        for w in std::mem::take(&mut self.retired) {
            Box::leak(w);
        }
        drain_default_context(self.gtk);
        for owned in [&mut self.session, &mut self.context] {
            if !owned.is_null() {
                // SAFETY: the object is ours (made by `create`), and every view that used it
                // was destroyed above and drained.
                unsafe { (self.gtk.fns.g_object_unref)(*owned) };
                *owned = std::ptr::null_mut();
            }
        }
        drain_default_context(self.gtk);
        // SAFETY: the loop was created by `new` and is released once.
        unsafe { (self.gtk.fns.g_main_loop_unref)(self.main_loop) };
    }
}

/// Cross-thread handle to the GLib main loop. Posting uses `g_idle_add`, which GLib makes safe
/// to call from any thread.
///
/// Support: Partial. Needs GTK 4 and WebKitGTK 6.0 at run time. Not run on macOS/Linux here;
/// compile-checked.
#[derive(Clone, Copy)]
pub struct Gtk4Proxy {
    gtk: &'static Gtk4,
}

impl EventLoopProxy for Gtk4Proxy {
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
/// `data` is the pointer `Gtk4Proxy::post` made from a boxed task, and runs exactly once.
unsafe extern "C" fn run_task(data: *mut c_void) -> c_int {
    // SAFETY: see the contract above.
    let task = unsafe { Box::from_raw(data.cast::<Box<dyn FnOnce() + Send>>()) };
    (*task)();
    0
}

impl ClipboardBackend for Gtk4Backend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        let clipboard = self.clipboard()?;
        let f = &self.gtk.fns;
        let (pending, data) = Pending::start();
        // SAFETY: the clipboard is live; `data` is a leaked `Rc` that `clipboard_read_ready`
        // takes back exactly once.
        unsafe {
            (f.gdk_clipboard_read_text_async)(
                clipboard,
                std::ptr::null_mut(),
                clipboard_read_ready as *const c_void,
                data,
            )
        };
        let Some(raw) = pending.wait(self.gtk, Some(CLIPBOARD_TIMEOUT)) else {
            return Err(Error::Backend(
                "the clipboard did not answer in time".into(),
            ));
        };
        if raw.is_null() {
            return Ok(None);
        }
        // SAFETY: GDK returned an owned string, freed here.
        let text = unsafe {
            let text = from_c(raw.cast::<c_char>());
            (f.g_free)(raw);
            text
        };
        Ok(Some(text))
    }

    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        let c = cstring(text)?;
        let clipboard = self.clipboard()?;
        let f = &self.gtk.fns;
        // SAFETY: the clipboard is live; GDK copies the text during the call.
        unsafe {
            (f.gdk_clipboard_set_text)(clipboard, c.as_ptr());
            // Keep the text after this process exits, as the other platforms do. The result is
            // not awaited: a missing clipboard manager only means the text lives as long as we do.
            (f.gdk_clipboard_store_async)(
                clipboard,
                sys::G_PRIORITY_DEFAULT,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
            );
        }
        Ok(())
    }
}

impl ThemeBackend for Gtk4Backend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        crate::dbus::settings::color_scheme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when GTK 4 and WebKitGTK 6.0 load and a display variable is set. Otherwise the tests
    /// below skip: they need a display (a desktop session, or Xvfb or Weston in CI).
    fn can_run_gtk() -> bool {
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        has_display && sys::load().is_ok()
    }

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "main".into(),
            title: "tpt test".into(),
            size: LogicalSize {
                width: 320.0,
                height: 240.0,
            },
            allow_navigation: vec![],
        }
    }

    #[test]
    fn without_a_display_the_adapter_says_so_and_does_not_crash() {
        if can_run_gtk() {
            return;
        }
        assert!(Gtk4Backend::new().is_err());
    }

    #[test]
    fn a_window_round_trip_under_a_display() {
        if !can_run_gtk() {
            eprintln!("skipped: no display or no GTK 4 and WebKitGTK 6.0 libraries");
            return;
        }
        let Some(_gui) = crate::testutil::gui_session("gtk4") else {
            return;
        };
        let mut backend = Gtk4Backend::new().expect("GTK starts when a display is present");
        let id = backend.create(&spec()).expect("a window is created");
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
        backend.set_frameless(id, true).expect("frameless");
        assert!(
            matches!(
                backend.set_always_on_top(id, true),
                Err(Error::Unsupported { .. })
            ),
            "GTK 4 has no keep-above, and says so"
        );
        assert!(matches!(backend.bounds(id), Err(Error::Unsupported { .. })));
        backend.close(id).expect("close");
        assert!(backend.state(id).is_err(), "a closed window is unknown");
    }

    #[test]
    fn the_proxy_runs_a_posted_task_on_the_loop() {
        if !can_run_gtk() {
            eprintln!("skipped: no display or no GTK 4 and WebKitGTK 6.0 libraries");
            return;
        }
        let Some(_gui) = crate::testutil::gui_session("gtk4") else {
            return;
        };
        let mut backend = Gtk4Backend::new().expect("GTK starts when a display is present");
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = std::sync::Arc::clone(&flag);
        backend
            .proxy()
            .post(Box::new(move || {
                seen.store(true, std::sync::atomic::Ordering::SeqCst)
            }))
            .expect("the task is queued");
        let ran = backend.run_until(
            || flag.load(std::sync::atomic::Ordering::SeqCst),
            Duration::from_secs(5),
        );
        assert!(ran, "the posted task ran on the main loop");
    }

    #[test]
    fn a_pending_call_that_finishes_after_the_wait_gave_up_is_freed() {
        // Pure: no GTK. A late callback must see live memory and run the free function.
        let (pending, data) = Pending::start();
        pending.abandoned.set(true);
        let freed = Cell::new(false);
        let mut marker = 0u8;
        // SAFETY: `data` came from `start` and is used once.
        unsafe {
            Pending::finish(data, (&mut marker as *mut u8).cast(), |_| freed.set(true));
        }
        assert!(freed.get());
        assert!(pending.done.get());
    }
}
