// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Backend trait set (spec §5.2). Platform crates implement these.
//!
//! This is the M1 subset: enough for a window with a webview to be created, shown,
//! loaded and messaged. Menus, tray, dialogs, clipboard and the rest of the
//! `Platform` trait are later milestones.

use crate::error::Result;
use crate::geometry::LogicalSize;

/// Settings used to create a window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub name: String,
    pub title: String,
    pub size: LogicalSize,
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

/// Window management.
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
}

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
}
