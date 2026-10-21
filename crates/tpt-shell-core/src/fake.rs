// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! In-memory backend for unit tests (spec §5.2). No OS calls. Every call is recorded in `log`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::backend::{
    Backdrop, CloseAction, EventLoopBackend, EventLoopProxy, WebviewBackend, WindowBackend,
    WindowSpec,
};
use crate::error::{Error, Result};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::platform::{ClipboardBackend, ColorScheme, ThemeBackend};

/// Window handle used by the fake backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct FakeWindow {
    pub spec: WindowSpec,
    pub visible: bool,
    pub fullscreen: bool,
    pub frameless: bool,
    pub always_on_top: bool,
    pub transparent: bool,
    pub backdrop: Backdrop,
    pub drag_regions: Vec<LogicalRect>,
    pub close_action: CloseAction,
    /// The owner window, for child windows.
    pub parent: Option<FakeId>,
    /// Whether this window accepts input. A modal child turns its owner off.
    pub enabled: bool,
    /// Whether this window is modal to its owner.
    pub modal: bool,
    pub html: Option<String>,
    pub url: Option<String>,
    pub messages: Vec<String>,
    pub closed: bool,
}

type Task = Box<dyn FnOnce() + Send>;

/// Proxy that queues tasks. `FakeBackend::drain` runs them.
#[derive(Clone, Default)]
pub struct FakeProxy {
    queue: Arc<Mutex<VecDeque<Task>>>,
}

impl EventLoopProxy for FakeProxy {
    fn post(&self, task: Task) -> Result<()> {
        self.queue
            .lock()
            .map_err(|_| Error::Backend("proxy queue poisoned".into()))?
            .push_back(task);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeBackend {
    windows: Vec<FakeWindow>,
    proxy: FakeProxy,
    pub quit_requested: bool,
    pub log: Vec<String>,
    /// Clipboard contents. `None` means the clipboard holds no text.
    pub clipboard: Option<String>,
    /// The system colour scheme the fake reports.
    pub scheme: ColorScheme,
}

impl ClipboardBackend for FakeBackend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        Ok(self.clipboard.clone())
    }

    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        self.clipboard = Some(text.to_string());
        Ok(())
    }
}

impl ThemeBackend for FakeBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        Ok(self.scheme)
    }
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn window(&self, id: FakeId) -> Result<&FakeWindow> {
        self.windows
            .get(id.0 as usize)
            .ok_or(Error::UnknownWindow(id.0))
    }

    fn window_mut(&mut self, id: FakeId) -> Result<&mut FakeWindow> {
        self.windows
            .get_mut(id.0 as usize)
            .ok_or(Error::UnknownWindow(id.0))
    }

    /// Simulates the user asking a window to close, and applies its close action.
    /// Returns the action taken.
    pub fn user_requests_close(&mut self, id: FakeId) -> Result<CloseAction> {
        let w = self.window_mut(id)?;
        let action = w.close_action;
        match action {
            CloseAction::Close => {
                w.closed = true;
                self.log.push(format!("close {}", id.0));
            }
            CloseAction::Hide => {
                w.visible = false;
                self.log.push(format!("hide {}", id.0));
            }
        }
        Ok(action)
    }

    /// Runs and clears every task posted through a proxy. Returns how many ran.
    pub fn drain(&self) -> usize {
        let tasks: Vec<Task> = match self.proxy.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => return 0,
        };
        let n = tasks.len();
        for t in tasks {
            t();
        }
        n
    }
}

impl EventLoopBackend for FakeBackend {
    type Proxy = FakeProxy;

    fn proxy(&self) -> FakeProxy {
        self.proxy.clone()
    }

    fn quit(&mut self) {
        self.quit_requested = true;
        self.log.push("quit".into());
    }
}

impl WindowBackend for FakeBackend {
    type Id = FakeId;

    fn create(&mut self, spec: &WindowSpec) -> Result<FakeId> {
        self.log.push(format!("create {}", spec.name));
        self.windows.push(FakeWindow {
            spec: spec.clone(),
            visible: false,
            fullscreen: false,
            frameless: false,
            always_on_top: false,
            transparent: false,
            backdrop: Backdrop::None,
            drag_regions: Vec::new(),
            close_action: CloseAction::Close,
            parent: None,
            enabled: true,
            modal: false,
            html: None,
            url: None,
            messages: Vec::new(),
            closed: false,
        });
        Ok(FakeId(self.windows.len() as u64 - 1))
    }

    fn show(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.visible = true;
        self.log.push(format!("show {}", id.0));
        Ok(())
    }

    fn hide(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.visible = false;
        self.log.push(format!("hide {}", id.0));
        Ok(())
    }

    fn set_title(&mut self, id: FakeId, title: &str) -> Result<()> {
        self.window_mut(id)?.spec.title = title.to_string();
        Ok(())
    }

    fn set_size(&mut self, id: FakeId, size: LogicalSize) -> Result<()> {
        self.window_mut(id)?.spec.size = size;
        Ok(())
    }

    fn set_fullscreen(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.fullscreen = on;
        Ok(())
    }

    fn close(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.closed = true;
        self.log.push(format!("close {}", id.0));
        Ok(())
    }

    fn set_frameless(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.frameless = on;
        Ok(())
    }

    fn set_always_on_top(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.always_on_top = on;
        Ok(())
    }

    fn set_transparent(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.transparent = on;
        Ok(())
    }

    fn set_backdrop(&mut self, id: FakeId, backdrop: Backdrop) -> Result<()> {
        self.window_mut(id)?.backdrop = backdrop;
        Ok(())
    }

    fn set_drag_regions(&mut self, id: FakeId, regions: &[LogicalRect]) -> Result<()> {
        self.window_mut(id)?.drag_regions = regions.to_vec();
        Ok(())
    }

    fn create_child(&mut self, parent: FakeId, spec: &WindowSpec) -> Result<FakeId> {
        self.window(parent)?;
        let id = self.create(spec)?;
        self.window_mut(id)?.parent = Some(parent);
        Ok(id)
    }

    fn set_modal(&mut self, child: FakeId, on: bool) -> Result<()> {
        let parent = self
            .window(child)?
            .parent
            .ok_or_else(|| Error::Backend("the window has no parent".into()))?;
        self.window_mut(child)?.modal = on;
        self.window_mut(parent)?.enabled = !on;
        Ok(())
    }

    fn set_close_action(&mut self, id: FakeId, action: CloseAction) -> Result<()> {
        self.window_mut(id)?.close_action = action;
        Ok(())
    }
}

impl WebviewBackend for FakeBackend {
    fn load_html(&mut self, id: FakeId, html: &str) -> Result<()> {
        let w = self.window_mut(id)?;
        w.html = Some(html.to_string());
        w.url = None;
        Ok(())
    }

    fn navigate(&mut self, id: FakeId, url: &str) -> Result<()> {
        if !url.starts_with("app://") {
            return Err(Error::Backend(format!("navigation blocked: {url}")));
        }
        let w = self.window_mut(id)?;
        w.url = Some(url.to_string());
        w.html = None;
        Ok(())
    }

    fn eval(&mut self, id: FakeId, script: &str) -> Result<()> {
        self.log.push(format!("eval {} {}", id.0, script));
        self.window(id).map(|_| ())
    }

    fn post_message(&mut self, id: FakeId, message: &str) -> Result<()> {
        self.window_mut(id)?.messages.push(message.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> WindowSpec {
        WindowSpec {
            name: name.into(),
            title: "t".into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    #[test]
    fn create_show_and_load() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        b.show(id).unwrap();
        b.load_html(id, "<p>hi</p>").unwrap();
        let w = b.window(id).unwrap();
        assert!(w.visible);
        assert_eq!(w.html.as_deref(), Some("<p>hi</p>"));
    }

    #[test]
    fn navigation_only_allows_app_scheme() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        assert!(b.navigate(id, "app://localhost/index.html").is_ok());
        assert!(b.navigate(id, "https://example.com").is_err());
        assert!(b.navigate(id, "file:///C:/secret.txt").is_err());
        assert_eq!(
            b.window(id).unwrap().url.as_deref(),
            Some("app://localhost/index.html")
        );
    }

    #[test]
    fn unknown_window_is_an_error() {
        let mut b = FakeBackend::new();
        assert_eq!(b.show(FakeId(7)), Err(Error::UnknownWindow(7)));
    }

    #[test]
    fn proxy_posts_run_on_drain() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let b = FakeBackend::new();
        let proxy = b.proxy();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            proxy
                .post(Box::new(move || {
                    h.fetch_add(1, Ordering::SeqCst);
                }))
                .unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(b.drain(), 1);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn child_windows_record_their_owner() {
        let mut b = FakeBackend::new();
        let main = b.create(&spec("main")).unwrap();
        let dialog = b.create_child(main, &spec("dialog")).unwrap();
        assert_eq!(b.window(dialog).unwrap().parent, Some(main));
        assert!(b.create_child(FakeId(99), &spec("x")).is_err());
    }

    #[test]
    fn a_modal_child_disables_its_owner_until_it_is_released() {
        let mut b = FakeBackend::new();
        let main = b.create(&spec("main")).unwrap();
        let dialog = b.create_child(main, &spec("dialog")).unwrap();
        b.set_modal(dialog, true).unwrap();
        assert!(!b.window(main).unwrap().enabled);
        b.set_modal(dialog, false).unwrap();
        assert!(b.window(main).unwrap().enabled);
    }

    #[test]
    fn modal_needs_an_owner() {
        let mut b = FakeBackend::new();
        let main = b.create(&spec("main")).unwrap();
        assert!(b.set_modal(main, true).is_err());
    }

    #[test]
    fn clipboard_and_theme_default_to_empty_and_light() {
        let mut b = FakeBackend::new();
        assert_eq!(b.clipboard_text(), Ok(None));
        assert_eq!(b.color_scheme(), Ok(ColorScheme::Light));
        b.set_clipboard_text("copied").unwrap();
        b.scheme = ColorScheme::Dark;
        assert_eq!(b.clipboard_text(), Ok(Some("copied".into())));
        assert_eq!(b.color_scheme(), Ok(ColorScheme::Dark));
    }

    #[test]
    fn quit_is_recorded() {
        let mut b = FakeBackend::new();
        b.quit();
        assert!(b.quit_requested);
    }

    #[test]
    fn window_chrome_settings_are_stored() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        b.set_frameless(id, true).unwrap();
        b.set_always_on_top(id, true).unwrap();
        b.set_transparent(id, true).unwrap();
        b.set_backdrop(id, Backdrop::Mica).unwrap();
        let region = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 32.0,
        };
        b.set_drag_regions(id, &[region]).unwrap();
        let w = b.window(id).unwrap();
        assert!(w.frameless && w.always_on_top && w.transparent);
        assert_eq!(w.backdrop, Backdrop::Mica);
        assert_eq!(w.drag_regions, vec![region]);
    }

    #[test]
    fn default_close_action_closes_and_hide_keeps_the_window() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        b.show(id).unwrap();
        assert_eq!(b.user_requests_close(id), Ok(CloseAction::Close));
        assert!(b.window(id).unwrap().closed);

        let kept = b.create(&spec("tray")).unwrap();
        b.show(kept).unwrap();
        b.set_close_action(kept, CloseAction::Hide).unwrap();
        assert_eq!(b.user_requests_close(kept), Ok(CloseAction::Hide));
        let w = b.window(kept).unwrap();
        assert!(!w.closed && !w.visible);
    }

    /// A backend that implements only the required methods.
    #[derive(Default)]
    struct Minimal;

    impl WindowBackend for Minimal {
        type Id = FakeId;
        fn create(&mut self, _: &WindowSpec) -> Result<FakeId> {
            Ok(FakeId(0))
        }
        fn show(&mut self, _: FakeId) -> Result<()> {
            Ok(())
        }
        fn hide(&mut self, _: FakeId) -> Result<()> {
            Ok(())
        }
        fn set_title(&mut self, _: FakeId, _: &str) -> Result<()> {
            Ok(())
        }
        fn set_size(&mut self, _: FakeId, _: LogicalSize) -> Result<()> {
            Ok(())
        }
        fn set_fullscreen(&mut self, _: FakeId, _: bool) -> Result<()> {
            Ok(())
        }
        fn close(&mut self, _: FakeId) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn optional_window_methods_default_to_unsupported() {
        let mut b = Minimal;
        let id = FakeId(0);
        assert_eq!(
            b.set_frameless(id, true),
            Err(Error::Unsupported {
                feature: "frameless"
            })
        );
        assert_eq!(
            b.set_close_action(id, CloseAction::Hide),
            Err(Error::Unsupported {
                feature: "close handling"
            })
        );
    }
}
