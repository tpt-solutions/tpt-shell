// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Backend trait set (spec §5.2). Platform crates implement these.
//!
//! This is the M1 subset: enough for a window with a webview to be created, shown,
//! loaded and messaged. Menus, tray, dialogs, clipboard and the rest of the
//! `Platform` trait are later milestones.

use crate::error::{Error, Result};
use crate::geometry::{LogicalRect, LogicalSize};

/// Settings used to create a window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub name: String,
    pub title: String,
    pub size: LogicalSize,
    /// `https://` URL prefixes this window may navigate to. Empty: app content only.
    pub allow_navigation: Vec<String>,
}

/// Owns the UI thread's event loop.
pub trait EventLoopBackend {
    /// Handle for posting work to the UI thread from any thread.
    type Proxy: EventLoopProxy;

    fn proxy(&self) -> Self::Proxy;

    /// Asks the loop to stop. Safe to call from the UI thread.
    fn quit(&mut self);
}

/// Cross-thread handle to the UI thread. The only way to touch UI from another thread.
pub trait EventLoopProxy: Send + Sync + Clone + 'static {
    /// Queues `task` to run on the UI thread.
    fn post(&self, task: Box<dyn FnOnce() + Send>) -> Result<()>;
}

/// What happens when the user asks a window to close (title bar button, Alt+F4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloseAction {
    /// Close the window.
    #[default]
    Close,
    /// Keep the window; hide it instead. The app can show it again later.
    Hide,
}

/// System backdrop material behind a window's client area (spec §10.x, Windows 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Backdrop {
    #[default]
    None,
    Mica,
    Acrylic,
    Tabbed,
}

/// Window management.
///
/// Methods past `close` are optional. Their default is [`Error::Unsupported`], so a backend
/// that does not provide one says so instead of silently ignoring the request.
pub trait WindowBackend {
    /// Backend-specific window handle.
    type Id: Copy + Eq + std::fmt::Debug;

    fn create(&mut self, spec: &WindowSpec) -> Result<Self::Id>;
    fn show(&mut self, id: Self::Id) -> Result<()>;
    fn hide(&mut self, id: Self::Id) -> Result<()>;
    fn set_title(&mut self, id: Self::Id, title: &str) -> Result<()>;
    fn set_size(&mut self, id: Self::Id, size: LogicalSize) -> Result<()>;
    fn set_fullscreen(&mut self, id: Self::Id, on: bool) -> Result<()>;
    fn close(&mut self, id: Self::Id) -> Result<()>;

    /// Removes the title bar and border.
    fn set_frameless(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "frameless",
        })
    }

    fn set_always_on_top(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "always-on-top",
        })
    }

    /// Makes the window background see-through where the page does not paint.
    fn set_transparent(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "transparency",
        })
    }

    fn set_backdrop(&mut self, _id: Self::Id, _backdrop: Backdrop) -> Result<()> {
        Err(Error::Unsupported {
            feature: "backdrop",
        })
    }

    /// Regions (in logical units, window coordinates) that drag the window when pressed.
    /// An empty slice removes all drag regions.
    fn set_drag_regions(&mut self, _id: Self::Id, _regions: &[LogicalRect]) -> Result<()> {
        Err(Error::Unsupported {
            feature: "drag regions",
        })
    }

    /// Chooses what a user close request does. Applies to later requests only.
    /// Creates a window owned by `parent`. It closes with its parent and stays above it.
    fn create_child(&mut self, _parent: Self::Id, _spec: &WindowSpec) -> Result<Self::Id> {
        Err(Error::Unsupported {
            feature: "child windows",
        })
    }

    /// Makes `child` modal to its owner: while on, the owner ignores input. Turning it off, or
    /// closing the child, gives the owner its input back.
    fn set_modal(&mut self, _child: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "modal windows",
        })
    }

    fn set_close_action(&mut self, _id: Self::Id, _action: CloseAction) -> Result<()> {
        Err(Error::Unsupported {
            feature: "close handling",
        })
    }
}

/// Receives a message the page posted, and returns the reply to post back, if there is one.
pub type MessageHandler = Box<dyn FnMut(&str) -> Option<String>>;

/// Webview inside a window.
pub trait WebviewBackend: WindowBackend {
    /// Loads an HTML document into the window's webview.
    fn load_html(&mut self, id: Self::Id, html: &str) -> Result<()>;
    /// Navigates to an `app://` URL. Other schemes are rejected by the navigation policy (spec §9).
    fn navigate(&mut self, id: Self::Id, url: &str) -> Result<()>;
    /// Runs a script in the page. Internal use only; not exposed to agents (spec §10.4).
    fn eval(&mut self, id: Self::Id, script: &str) -> Result<()>;
    /// Sends a message string to the page's `message` listener.
    fn post_message(&mut self, id: Self::Id, message: &str) -> Result<()>;
    /// Sets the handler for messages the page posts to the shell. A reply the handler returns is
    /// posted back to the page. Replaces any earlier handler. Backends that cannot receive page
    /// messages yet return [`Error::Unsupported`].
    fn set_message_handler(&mut self, id: Self::Id, handler: MessageHandler) -> Result<()> {
        let _ = (id, handler);
        Err(Error::Unsupported {
            feature: "page messages",
        })
    }
}
