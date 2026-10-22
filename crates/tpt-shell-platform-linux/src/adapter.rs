// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Linux window backend the shell uses: the GTK 3 adapter or the GTK 4 adapter, chosen when
//! the backend is created (see [`crate::probe`] for the rule and the `TPT_GTK` override).
//!
//! [`LinuxBackend`] is an enum over the two adapters. Both use the same window handle
//! ([`WindowId`]), so the enum implements the same `tpt-shell-core` traits and a caller does not
//! care which toolkit runs. Every method forwards to the adapter; nothing here calls GTK.
//!
//! Support: Partial. Whatever the chosen adapter supports (see [`crate::gtk3`] and
//! [`crate::gtk4`]).

use std::ffi::c_int;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tpt_shell_core::backend::{
    AssetSource, CloseAction, EventLoopBackend, EventLoopProxy, MessageHandler, WebviewBackend,
    WindowBackend, WindowSpec,
};
use tpt_shell_core::platform::{ClipboardBackend, ColorScheme, ThemeBackend};
use tpt_shell_core::{Error, LogicalRect, LogicalSize, Result};

use crate::gtk3::dialogs::FileDialog;
use crate::gtk3::{Gtk3Backend, Gtk3Proxy, WindowId};
use crate::gtk4::{Gtk4Backend, Gtk4Proxy};
use crate::probe::{self, Adapter};

/// The GTK 3 or GTK 4 backend, whichever was selected.
///
/// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
/// compile-checked.
pub enum LinuxBackend {
    Gtk3(Gtk3Backend),
    Gtk4(Gtk4Backend),
}

/// Runs `$body` with `$b` bound to the adapter inside `$self`.
macro_rules! each {
    ($self:expr, $b:ident => $body:expr) => {
        match $self {
            LinuxBackend::Gtk3($b) => $body,
            LinuxBackend::Gtk4($b) => $body,
        }
    };
}

impl LinuxBackend {
    /// Selects an adapter (GTK 4 if both load; `TPT_GTK=3|4` overrides) and starts it on this
    /// thread. When neither can load, or the requested one cannot, the error says which library
    /// is missing and which packages to install.
    ///
    /// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn new() -> Result<LinuxBackend> {
        let adapter = probe::select_from_env().map_err(Error::Backend)?;
        LinuxBackend::with_adapter(adapter)
    }

    /// Starts a specific adapter, ignoring `TPT_GTK`.
    ///
    /// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn with_adapter(adapter: Adapter) -> Result<LinuxBackend> {
        Ok(match adapter {
            Adapter::Gtk3 => LinuxBackend::Gtk3(Gtk3Backend::new()?),
            Adapter::Gtk4 => LinuxBackend::Gtk4(Gtk4Backend::new()?),
        })
    }

    /// Which adapter is running.
    ///
    /// Support: Full.
    pub fn adapter(&self) -> Adapter {
        match self {
            LinuxBackend::Gtk3(_) => Adapter::Gtk3,
            LinuxBackend::Gtk4(_) => Adapter::Gtk4,
        }
    }

    /// Runs the toolkit's main loop until [`EventLoopBackend::quit`] is called.
    ///
    /// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn run(&mut self) {
        each!(self, b => b.run())
    }

    /// Runs the main loop's pending work until `done` is true or `timeout` passes. Returns
    /// whether `done` became true.
    ///
    /// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn run_until(&mut self, mut done: impl FnMut() -> bool, timeout: Duration) -> bool {
        match self {
            LinuxBackend::Gtk4(b) => b.run_until(done, timeout),
            LinuxBackend::Gtk3(b) => {
                let deadline = Instant::now() + timeout;
                loop {
                    if done() {
                        return true;
                    }
                    if Instant::now() > deadline {
                        return false;
                    }
                    // SAFETY: a non-blocking iteration of the GTK main loop, on the thread that
                    // started GTK (the backend is not Send).
                    let ran: c_int = unsafe { (b.gtk.fns.gtk_main_iteration_do)(0) };
                    if ran == 0 {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            }
        }
    }

    /// Asks the user to pick one or more existing files. `None` when cancelled.
    ///
    /// Support: Partial. GTK 3 always; GTK 4 needs 4.10 or later (else `Unsupported`).
    pub fn open_files(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<Vec<PathBuf>>> {
        each!(self, b => b.open_files(parent, dialog))
    }

    /// Asks the user where to save a file. `None` when cancelled.
    ///
    /// Support: Partial. GTK 3 always; GTK 4 needs 4.10 or later (else `Unsupported`).
    pub fn save_file(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<PathBuf>> {
        each!(self, b => b.save_file(parent, dialog))
    }

    /// Window position and size. GTK 3 only: GTK 4 has no window position, so it returns
    /// `Unsupported` (use the size through `WindowBackend::set_size`).
    ///
    /// Support: Partial. GTK 3 only; Unsupported on GTK 4.
    pub fn bounds(&self, id: WindowId) -> Result<LogicalRect> {
        each!(self, b => b.bounds(id))
    }

    /// Moves and resizes the window. GTK 3 only (see [`LinuxBackend::bounds`]).
    ///
    /// Support: Partial. GTK 3 only; Unsupported on GTK 4.
    pub fn set_bounds(&mut self, id: WindowId, rect: LogicalRect) -> Result<()> {
        each!(self, b => b.set_bounds(id, rect))
    }
}

impl WindowBackend for LinuxBackend {
    type Id = WindowId;

    fn create(&mut self, spec: &WindowSpec) -> Result<WindowId> {
        each!(self, b => b.create(spec))
    }
    fn show(&mut self, id: WindowId) -> Result<()> {
        each!(self, b => b.show(id))
    }
    fn hide(&mut self, id: WindowId) -> Result<()> {
        each!(self, b => b.hide(id))
    }
    fn set_title(&mut self, id: WindowId, title: &str) -> Result<()> {
        each!(self, b => b.set_title(id, title))
    }
    fn set_size(&mut self, id: WindowId, size: LogicalSize) -> Result<()> {
        each!(self, b => b.set_size(id, size))
    }
    fn set_fullscreen(&mut self, id: WindowId, on: bool) -> Result<()> {
        each!(self, b => b.set_fullscreen(id, on))
    }
    fn close(&mut self, id: WindowId) -> Result<()> {
        each!(self, b => b.close(id))
    }
    fn set_frameless(&mut self, id: WindowId, on: bool) -> Result<()> {
        each!(self, b => b.set_frameless(id, on))
    }
    fn set_always_on_top(&mut self, id: WindowId, on: bool) -> Result<()> {
        each!(self, b => b.set_always_on_top(id, on))
    }
    fn set_transparent(&mut self, id: WindowId, on: bool) -> Result<()> {
        each!(self, b => b.set_transparent(id, on))
    }
    fn set_close_action(&mut self, id: WindowId, action: CloseAction) -> Result<()> {
        each!(self, b => b.set_close_action(id, action))
    }
}

impl WebviewBackend for LinuxBackend {
    fn load_html(&mut self, id: WindowId, html: &str) -> Result<()> {
        each!(self, b => b.load_html(id, html))
    }
    fn navigate(&mut self, id: WindowId, url: &str) -> Result<()> {
        each!(self, b => b.navigate(id, url))
    }
    fn eval(&mut self, id: WindowId, script: &str) -> Result<()> {
        each!(self, b => b.eval(id, script))
    }
    fn post_message(&mut self, id: WindowId, message: &str) -> Result<()> {
        each!(self, b => b.post_message(id, message))
    }
    fn set_message_handler(&mut self, id: WindowId, handler: MessageHandler) -> Result<()> {
        each!(self, b => b.set_message_handler(id, handler))
    }
    fn set_asset_source(&mut self, id: WindowId, source: AssetSource) -> Result<()> {
        each!(self, b => b.set_asset_source(id, source))
    }
    fn snapshot(&mut self, id: WindowId) -> Result<Vec<u8>> {
        each!(self, b => b.snapshot_png(id))
    }
}

/// Cross-thread handle to whichever main loop is running.
///
/// Support: Partial. Needs GTK and WebKitGTK at run time. Not run on macOS/Linux here;
/// compile-checked.
#[derive(Clone, Copy)]
pub enum LinuxProxy {
    Gtk3(Gtk3Proxy),
    Gtk4(Gtk4Proxy),
}

impl EventLoopProxy for LinuxProxy {
    fn post(&self, task: Box<dyn FnOnce() + Send>) -> Result<()> {
        match self {
            LinuxProxy::Gtk3(p) => p.post(task),
            LinuxProxy::Gtk4(p) => p.post(task),
        }
    }
}

impl EventLoopBackend for LinuxBackend {
    type Proxy = LinuxProxy;

    fn proxy(&self) -> LinuxProxy {
        match self {
            LinuxBackend::Gtk3(b) => LinuxProxy::Gtk3(b.proxy()),
            LinuxBackend::Gtk4(b) => LinuxProxy::Gtk4(b.proxy()),
        }
    }

    fn quit(&mut self) {
        each!(self, b => b.quit())
    }
}

impl ClipboardBackend for LinuxBackend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        each!(self, b => b.clipboard_text())
    }
    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        each!(self, b => b.set_clipboard_text(text))
    }
}

impl ThemeBackend for LinuxBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        each!(self, b => b.color_scheme())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::testutil::{gui_session, skip_or_fail};
    use tpt_shell_core::backend::Asset;

    /// The adapter this run can exercise: the one `TPT_GTK` asks for, else GTK 4 if it loads,
    /// else GTK 3. `Err` says why not: no display, or no adapter can load.
    fn available() -> std::result::Result<Adapter, String> {
        let has_display = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()));
        if !has_display {
            return Err("no DISPLAY or WAYLAND_DISPLAY".into());
        }
        probe::select_from_env().map_err(|e| format!("no usable adapter: {e}"))
    }

    /// Like [`available`], but a missing display or adapter is a skip with a message (or a
    /// failure under `TPT_LINUX_STRICT`). CI runs this crate once per adapter with only that
    /// adapter's packages installed.
    fn live() -> Option<Adapter> {
        match available() {
            Ok(adapter) => Some(adapter),
            Err(why) => {
                skip_or_fail(&why);
                None
            }
        }
    }

    fn name(adapter: Adapter) -> &'static str {
        match adapter {
            Adapter::Gtk3 => "gtk3",
            Adapter::Gtk4 => "gtk4",
        }
    }

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "main".into(),
            title: "tpt adapter test".into(),
            size: LogicalSize {
                width: 320.0,
                height: 240.0,
            },
            allow_navigation: vec![],
        }
    }

    #[test]
    fn with_no_display_or_libraries_creation_fails_with_a_message() {
        if available().is_ok() {
            return;
        }
        // Nothing can run here, so creation must be an error that says why, not a panic.
        match LinuxBackend::new() {
            Err(e) => assert!(!e.to_string().is_empty()),
            Ok(_) => panic!("a backend started although nothing can run"),
        }
    }

    #[test]
    fn a_window_round_trip_through_the_selected_adapter() {
        let Some(adapter) = live() else { return };
        let Some(_gui) = gui_session(name(adapter)) else {
            return;
        };
        let mut backend = LinuxBackend::with_adapter(adapter).expect("the adapter starts");
        assert_eq!(backend.adapter(), adapter);
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
        backend.show(id).expect("show");
        backend.hide(id).expect("hide");
        backend.close(id).expect("close");
        assert!(backend.hide(id).is_err(), "a closed window is unknown");
    }

    #[test]
    fn a_posted_task_runs_on_the_main_loop() {
        let Some(adapter) = live() else { return };
        let Some(_gui) = gui_session(name(adapter)) else {
            return;
        };
        let mut backend = LinuxBackend::with_adapter(adapter).expect("the adapter starts");
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = std::sync::Arc::clone(&flag);
        backend
            .proxy()
            .post(Box::new(move || {
                seen.store(true, std::sync::atomic::Ordering::SeqCst)
            }))
            .expect("queued");
        assert!(backend.run_until(
            || flag.load(std::sync::atomic::Ordering::SeqCst),
            Duration::from_secs(5)
        ));
    }

    #[test]
    fn the_clipboard_round_trips_on_x11() {
        let Some(adapter) = live() else { return };
        let Some(_gui) = gui_session(name(adapter)) else {
            return;
        };
        // A Wayland compositor hands the clipboard only to a focused client, and a headless
        // compositor has no focus, so the clipboard is not exercised there at all (a read could
        // wait for a selection that never arrives).
        let on_x11 = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            && std::env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty());
        if !on_x11 {
            eprintln!("skipped: the clipboard is only tested on a plain X11 display");
            return;
        }
        let mut backend = LinuxBackend::with_adapter(adapter).expect("the adapter starts");
        let text = "tpt-shell clipboard test";
        backend
            .set_clipboard_text(text)
            .expect("the clipboard accepts text");
        assert_eq!(
            backend.clipboard_text().expect("the clipboard answers"),
            Some(text.to_string())
        );
    }

    /// Needs the web process to start. In a sandbox that forbids it (no user namespaces), the
    /// page never posts; that is a skip with a message, unless `TPT_LINUX_STRICT` is set.
    #[test]
    fn page_messages_assets_and_snapshot_through_the_selected_adapter() {
        let Some(adapter) = live() else { return };
        let Some(_gui) = gui_session(name(adapter)) else {
            return;
        };
        let mut backend = LinuxBackend::with_adapter(adapter).expect("the adapter starts");
        let id = backend.create(&spec()).expect("a window is created");

        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&log);
        backend
            .set_message_handler(
                id,
                Box::new(move |text| {
                    seen.borrow_mut().push(text.to_string());
                    (text == "asset-ok").then(|| "pong".to_string())
                }),
            )
            .expect("handler");
        backend
            .set_asset_source(
                id,
                Box::new(|key| {
                    (key == "index.html").then(|| Asset {
                        content_type: "text/html".into(),
                        body: br#"<!doctype html><title>t</title><body style="background:#336">
<script>
window.addEventListener('message', function (e) {
  window.chrome.webview.postMessage('got:' + e.data);
});
window.chrome.webview.postMessage('asset-ok');
</script></body>"#
                            .to_vec(),
                    })
                }),
            )
            .expect("asset source");
        backend
            .navigate(id, "app://tpt-app/index.html")
            .expect("app pages are always allowed");
        backend.show(id).expect("show");

        let arrived = backend.run_until(
            || log.borrow().iter().any(|m| m == "got:pong"),
            Duration::from_secs(30),
        );
        if !arrived {
            skip_or_fail(&format!(
                "the page did not complete the message round trip in 30 s (log: {:?}); the web \
                 process may not be able to start here",
                log.borrow()
            ));
            let _ = backend.close(id);
            return;
        }
        assert_eq!(
            log.borrow().first().map(String::as_str),
            Some("asset-ok"),
            "the page came from the app:// scheme and posted first"
        );

        // A navigation the policy does not allow is refused before WebKit sees it.
        assert!(backend.navigate(id, "https://example.invalid/").is_err());

        match backend.snapshot(id) {
            Ok(png) => assert!(
                png.starts_with(&[0x89, b'P', b'N', b'G']),
                "the snapshot is a PNG"
            ),
            Err(e) => skip_or_fail(&format!("snapshot not available here: {e}")),
        }
        backend.close(id).expect("close");
    }
}
