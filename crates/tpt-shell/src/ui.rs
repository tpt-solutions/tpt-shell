// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A handle that other threads use to ask the UI thread to work on the app's windows (Windows).
//!
//! The handle wraps the backend's event-loop proxy (`EventLoopProxy`). Each call queues a task
//! that runs on the UI thread, so it returns before the work is done. A failure in the task is
//! reported on stderr, because nothing waits for it. A window name that the app did not open is
//! refused at once.

use std::sync::Arc;

use serde_json::Value;
use tpt_shell_core::backend::EventLoopProxy;
use tpt_shell_core::{Error, Result};
use tpt_shell_ipc::event;
use tpt_shell_platform_windows::backend::{
    eval_window, post_window_message, show_window, WinProxy, WindowId,
};

/// Sends work to the UI thread from any thread. Clone it freely. Obtain one with
/// `Running::ui_handle`.
///
/// Support: Full on Windows.
#[derive(Clone)]
pub struct UiHandle {
    proxy: WinProxy,
    windows: Arc<Vec<(String, WindowId)>>,
}

impl UiHandle {
    pub(crate) fn new(proxy: WinProxy, windows: Vec<(String, WindowId)>) -> Self {
        UiHandle {
            proxy,
            windows: Arc::new(windows),
        }
    }

    /// Shows the window called `window` on the UI thread, if it is hidden.
    ///
    /// Support: Full on Windows.
    pub fn show(&self, window: &str) -> Result<()> {
        let id = self.find(window)?;
        self.post(move || show_window(id))
    }

    /// Sends the event `name` with `data` to the page of `window`. The page gets it through
    /// `window.tpt.on(name, handler)`, as with `Running::emit`. Events are not queued.
    ///
    /// Support: Full on Windows.
    pub fn emit(&self, window: &str, name: &str, data: Value) -> Result<()> {
        let id = self.find(window)?;
        let message = event(name, data).to_string();
        self.post(move || post_window_message(id, &message))
    }

    /// Sends one rendered frame to the page of `window` as the event `viewport.frame`, for a
    /// `<canvas>` to draw. `rgba` is `width * height * 4` bytes, row-major RGBA8 from the top
    /// row. See the `viewport` module for the payload.
    ///
    /// Support: Full on Windows.
    pub fn viewport_frame(&self, window: &str, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
        let data = crate::viewport::frame_data(width, height, rgba)?;
        self.emit(window, crate::viewport::FRAME_EVENT, data)
    }

    /// Like [`UiHandle::viewport_frame`], with the binding `name` in the event, so a page with
    /// several viewports draws each frame into the canvas that has that `data-viewport` name.
    ///
    /// Support: Full on Windows.
    pub fn viewport_frame_named(
        &self,
        window: &str,
        name: &str,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<()> {
        let data = crate::viewport::frame_data_named(name, width, height, rgba)?;
        self.emit(window, crate::viewport::FRAME_EVENT, data)
    }

    /// Runs `script` in the page of `window` on the UI thread, without waiting for it. Internal
    /// use only: the script runs with the page's own rights, so only the app's own code calls it.
    ///
    /// Support: Partial. Errors thrown by the script are not reported.
    pub fn eval(&self, window: &str, script: &str) -> Result<()> {
        let id = self.find(window)?;
        let script = script.to_string();
        self.post(move || eval_window(id, &script))
    }

    fn find(&self, window: &str) -> Result<WindowId> {
        self.windows
            .iter()
            .find(|(name, _)| name == window)
            .map(|(_, id)| *id)
            .ok_or_else(|| Error::Backend(format!("no window named {window:?}")))
    }

    fn post(&self, work: impl FnOnce() -> Result<()> + Send + 'static) -> Result<()> {
        self.proxy.post(Box::new(move || {
            if let Err(e) = work() {
                eprintln!("tpt-shell: a call from another thread failed: {e}");
            }
        }))
    }
}

/// Compile-time check that the handle can be moved and shared between threads.
#[allow(dead_code)]
fn assert_send_sync() {
    fn check<T: Send + Sync + Clone + 'static>() {}
    check::<UiHandle>();
}
