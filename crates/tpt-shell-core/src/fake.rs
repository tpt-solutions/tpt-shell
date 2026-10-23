// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! In-memory backend for unit tests (spec §5.2). No OS calls. Every call is recorded in `log`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use std::path::PathBuf;

use crate::backend::{
    devtools_policy, AssetSource, Backdrop, CloseAction, EventLoopBackend, EventLoopProxy,
    MessageHandler, PreferredScheme, WebviewBackend, WindowBackend, WindowSpec,
};
use crate::error::{Error, Result};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::platform::{
    ClipboardBackend, ColorScheme, MessageAnswer, MessageBox, OpenDialog, Platform, ThemeBackend,
};

/// The PNG signature, which the fake's snapshots start with.
///
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
pub const FAKE_PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Window handle used by the fake backend.
///
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeId(pub u64);
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.

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
    pub dark_title_bar: bool,
    /// Whether the developer tools are on for this window's page.
    pub devtools: bool,
    /// How many times the developer tools were opened in their own window.
    pub devtools_opened: u32,
    pub preferred_scheme: PreferredScheme,
}

type Task = Box<dyn FnOnce() + Send>;

/// Proxy that queues tasks. `FakeBackend::drain` runs them.
///
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
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
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.

#[derive(Default)]
pub struct FakeBackend {
    windows: Vec<FakeWindow>,
    /// Page message handlers, by window index.
    page_handlers: HashMap<u64, MessageHandler>,
    /// App asset sources, by window index.
    asset_sources: HashMap<u64, AssetSource>,
    proxy: FakeProxy,
    pub quit_requested: bool,
    pub log: Vec<String>,
    /// Clipboard contents. `None` means the clipboard holds no text.
    pub clipboard: Option<String>,
    /// The system colour scheme the fake reports.
    pub scheme: ColorScheme,
    /// Whether the fake behaves as a release build (developer tools refused).
    pub release_build: bool,
    /// Answers the next message boxes give, oldest first. With none left, a box is cancelled.
    pub message_answers: VecDeque<MessageAnswer>,
    /// Paths the next open dialogs return, oldest first. With none left, the dialog is cancelled.
    pub open_answers: VecDeque<Vec<PathBuf>>,
    /// Message boxes shown, in order.
    pub messages_shown: Vec<MessageBox>,
    /// Secrets in the fake keystore.
    pub keystore: HashMap<String, Vec<u8>>,
    /// URL schemes registered, in order.
    pub schemes: Vec<String>,
    /// Notifications shown, as (title, body).
    pub notifications: Vec<(String, String)>,
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
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
    pub fn new() -> Self {
        Self::default()
    }
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.

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
    ///
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
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
    ///
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
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
            dark_title_bar: false,
            devtools: !self.release_build,
            devtools_opened: 0,
            preferred_scheme: PreferredScheme::System,
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

    fn set_dark_title_bar(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.dark_title_bar = on;
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

    fn set_message_handler(&mut self, id: FakeId, handler: MessageHandler) -> Result<()> {
        self.window(id)?;
        self.page_handlers.insert(id.0, handler);
        Ok(())
    }

    fn set_asset_source(&mut self, id: FakeId, source: AssetSource) -> Result<()> {
        self.window(id)?;
        self.asset_sources.insert(id.0, source);
        Ok(())
    }

    fn snapshot(&mut self, id: FakeId) -> Result<Vec<u8>> {
        self.window(id)?;
        Ok(FAKE_PNG.to_vec())
    }

    fn set_devtools(&mut self, id: FakeId, on: bool) -> Result<()> {
        devtools_policy(self.release_build, on)?;
        self.window_mut(id)?.devtools = on;
        Ok(())
    }

    fn open_devtools(&mut self, id: FakeId) -> Result<()> {
        let w = self.window_mut(id)?;
        if !w.devtools {
            return Err(Error::Unsupported {
                feature: "developer tools are off for this window",
            });
        }
        w.devtools_opened += 1;
        Ok(())
    }

    fn set_preferred_color_scheme(&mut self, id: FakeId, scheme: PreferredScheme) -> Result<()> {
        self.window_mut(id)?.preferred_scheme = scheme;
        Ok(())
    }
}

impl Platform for FakeBackend {
    fn message_box(
        &mut self,
        owner: Option<FakeId>,
        message: &MessageBox,
    ) -> Result<MessageAnswer> {
        if let Some(owner) = owner {
            self.window(owner)?;
        }
        self.messages_shown.push(message.clone());
        Ok(self
            .message_answers
            .pop_front()
            .unwrap_or(MessageAnswer::Cancel))
    }

    fn open_files(&mut self, owner: Option<FakeId>, dialog: &OpenDialog) -> Result<Vec<PathBuf>> {
        if let Some(owner) = owner {
            self.window(owner)?;
        }
        let picked = self.open_answers.pop_front().unwrap_or_default();
        if !dialog.multiple && picked.len() > 1 {
            return Err(Error::Backend(
                "a single-select dialog returned several files".into(),
            ));
        }
        Ok(picked)
    }

    fn keystore_set(&mut self, name: &str, secret: &[u8]) -> Result<()> {
        self.keystore.insert(name.to_string(), secret.to_vec());
        Ok(())
    }

    fn keystore_get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.keystore.get(name).cloned())
    }

    fn keystore_delete(&mut self, name: &str) -> Result<bool> {
        Ok(self.keystore.remove(name).is_some())
    }

    fn register_scheme(&mut self, scheme: &str, _description: &str) -> Result<()> {
        if !self.schemes.iter().any(|s| s == scheme) {
            self.schemes.push(scheme.to_string());
        }
        Ok(())
    }

    fn unregister_scheme(&mut self, scheme: &str) -> Result<()> {
        self.schemes.retain(|s| s != scheme);
        Ok(())
    }

    fn notify(&mut self, title: &str, body: &str) -> Result<()> {
        self.notifications
            .push((title.to_string(), body.to_string()));
        Ok(())
    }
}

impl FakeBackend {
    /// What the window's asset source returns for `path`, as the page would receive it.
    ///
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
    pub fn fetch_asset(&self, id: FakeId, path: &str) -> Result<Option<crate::backend::Asset>> {
        self.window(id)?;
        Ok(self
            .asset_sources
            .get(&id.0)
            .and_then(|source| source(path)))
    }

    /// Simulates the page posting `text` to the shell. The window's handler runs, and its reply
    /// (if any) is recorded as a message to the page, as the real backend would post it.
    ///
    /// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
    pub fn page_sends(&mut self, id: FakeId, text: &str) -> Result<Option<String>> {
        self.window(id)?;
        let reply = match self.page_handlers.get_mut(&id.0) {
            Some(handler) => handler(text),
            None => return Ok(None),
        };
        if let Some(reply) = &reply {
            self.post_message(id, reply)?;
        }
        Ok(reply)
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

#[cfg(test)]
mod page_message_tests {
    use super::*;

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "main".into(),
            title: "t".into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    #[test]
    fn the_handlers_reply_is_posted_back_to_the_page() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        b.set_message_handler(id, Box::new(|text| Some(format!("re:{text}"))))
            .unwrap();
        assert_eq!(b.page_sends(id, "hi").unwrap(), Some("re:hi".to_string()));
        assert_eq!(b.window(id).unwrap().messages, ["re:hi"]);
    }

    #[test]
    fn a_handler_that_declines_to_reply_posts_nothing() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        b.set_message_handler(id, Box::new(|_| None)).unwrap();
        assert_eq!(b.page_sends(id, "hi").unwrap(), None);
        assert!(b.window(id).unwrap().messages.is_empty());
    }

    #[test]
    fn a_window_without_a_handler_ignores_page_messages() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        assert_eq!(b.page_sends(id, "hi").unwrap(), None);
    }

    #[test]
    fn a_later_handler_replaces_the_earlier_one() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        b.set_message_handler(id, Box::new(|_| Some("first".into())))
            .unwrap();
        b.set_message_handler(id, Box::new(|_| Some("second".into())))
            .unwrap();
        assert_eq!(b.page_sends(id, "x").unwrap(), Some("second".to_string()));
    }

    #[test]
    fn an_unknown_window_is_refused() {
        let mut b = FakeBackend::new();
        assert!(b
            .set_message_handler(FakeId(9), Box::new(|_| None))
            .is_err());
        assert!(b.page_sends(FakeId(9), "x").is_err());
    }
}

#[cfg(test)]
mod asset_tests {
    use super::*;
    use crate::backend::Asset;

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "main".into(),
            title: "t".into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    #[test]
    fn the_asset_source_answers_for_its_window() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        b.set_asset_source(
            id,
            Box::new(|path| {
                (path == "index.html").then(|| Asset {
                    content_type: "text/html".into(),
                    body: b"<p>hi</p>".to_vec(),
                })
            }),
        )
        .unwrap();
        let found = b.fetch_asset(id, "index.html").unwrap().unwrap();
        assert_eq!(found.content_type, "text/html");
        assert_eq!(b.fetch_asset(id, "missing.js").unwrap(), None);
    }

    #[test]
    fn a_window_without_a_source_has_no_assets() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        assert_eq!(b.fetch_asset(id, "index.html").unwrap(), None);
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "main".into(),
            title: "Main".into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    #[test]
    fn message_boxes_return_queued_answers_and_cancel_when_none_are_left() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        b.message_answers.push_back(MessageAnswer::Yes);
        let question = MessageBox {
            title: "Save".into(),
            text: "Keep changes?".into(),
            buttons: crate::platform::MessageButtons::YesNo,
            icon: crate::platform::MessageIcon::Warning,
        };
        assert_eq!(
            b.message_box(Some(id), &question).unwrap(),
            MessageAnswer::Yes
        );
        assert_eq!(
            b.message_box(None, &question).unwrap(),
            MessageAnswer::Cancel
        );
        assert_eq!(b.messages_shown.len(), 2);
    }

    #[test]
    fn a_message_box_needs_a_real_owner() {
        let mut b = FakeBackend::new();
        let bogus = FakeId(9);
        assert_eq!(
            b.message_box(Some(bogus), &MessageBox::default()),
            Err(Error::UnknownWindow(9))
        );
    }

    #[test]
    fn open_dialog_returns_the_queued_paths_and_refuses_several_in_single_mode() {
        let mut b = FakeBackend::new();
        let single = OpenDialog {
            title: "Open".into(),
            filters: Vec::new(),
            multiple: false,
        };
        let many = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        b.open_answers.push_back(many.clone());
        assert!(b.open_files(None, &single).is_err());
        b.open_answers.push_back(many.clone());
        let multi = OpenDialog {
            multiple: true,
            ..single
        };
        assert_eq!(b.open_files(None, &multi).unwrap(), many);
        assert!(b.open_files(None, &multi).unwrap().is_empty());
    }

    #[test]
    fn keystore_round_trips_and_delete_reports_what_was_there() {
        let mut b = FakeBackend::new();
        assert_eq!(b.keystore_get("token").unwrap(), None);
        b.keystore_set("token", b"secret").unwrap();
        assert_eq!(b.keystore_get("token").unwrap(), Some(b"secret".to_vec()));
        assert!(b.keystore_delete("token").unwrap());
        assert!(!b.keystore_delete("token").unwrap());
    }

    #[test]
    fn schemes_and_notifications_are_recorded() {
        let mut b = FakeBackend::new();
        b.register_scheme("myapp", "My app").unwrap();
        b.register_scheme("myapp", "My app").unwrap();
        assert_eq!(b.schemes, vec!["myapp".to_string()]);
        b.unregister_scheme("myapp").unwrap();
        assert!(b.schemes.is_empty());
        b.notify("Done", "Export finished").unwrap();
        assert_eq!(
            b.notifications,
            vec![("Done".into(), "Export finished".into())]
        );
    }

    #[test]
    fn release_builds_refuse_devtools_and_debug_builds_allow_them() {
        let mut debug = FakeBackend::new();
        let id = debug.create(&spec()).unwrap();
        assert!(debug.window(id).unwrap().devtools);
        debug.set_devtools(id, false).unwrap();
        debug.set_devtools(id, true).unwrap();
        debug.open_devtools(id).unwrap();
        assert_eq!(debug.window(id).unwrap().devtools_opened, 1);

        let mut release = FakeBackend {
            release_build: true,
            ..FakeBackend::new()
        };
        let id = release.create(&spec()).unwrap();
        assert!(!release.window(id).unwrap().devtools);
        assert!(release.set_devtools(id, true).is_err());
        assert!(!release.window(id).unwrap().devtools);
        assert!(release.open_devtools(id).is_err());
        assert_eq!(release.window(id).unwrap().devtools_opened, 0);
        release.set_devtools(id, false).unwrap();
    }

    #[test]
    fn snapshot_preferred_scheme_and_dark_title_bar_are_stored() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec()).unwrap();
        assert_eq!(b.snapshot(id).unwrap(), FAKE_PNG);
        b.set_preferred_color_scheme(id, PreferredScheme::Dark)
            .unwrap();
        b.set_dark_title_bar(id, true).unwrap();
        let w = b.window(id).unwrap();
        assert_eq!(w.preferred_scheme, PreferredScheme::Dark);
        assert!(w.dark_title_bar);
        assert!(b.snapshot(FakeId(7)).is_err());
    }
}
