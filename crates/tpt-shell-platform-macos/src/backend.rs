// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Window and webview operations of the macOS backend, behind the core traits.
//!
//! Every method that is not written yet returns [`Error::Unsupported`]. That keeps the gaps
//! visible to callers instead of silently doing nothing.

use std::collections::HashMap;

use objc2::MainThreadMarker;
use tpt_shell_core::backend::{WebviewBackend, WindowBackend, WindowSpec};
use tpt_shell_core::{Error, LogicalSize, Result};

use crate::window::MacWindow;

/// Handle to a window managed by [`MacBackend`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowId(pub usize);

/// Owns the windows. Create it on the main thread.
pub struct MacBackend {
    mtm: MainThreadMarker,
    next: usize,
    windows: HashMap<usize, MacWindow>,
}

impl MacBackend {
    /// `None` when the caller is not on the main thread.
    pub fn new() -> Option<Self> {
        MainThreadMarker::new().map(|mtm| MacBackend {
            mtm,
            next: 1,
            windows: HashMap::new(),
        })
    }

    fn window(&self, id: WindowId) -> Result<&MacWindow> {
        self.windows
            .get(&id.0)
            .ok_or(Error::UnknownWindow(id.0 as u64))
    }
}

impl WindowBackend for MacBackend {
    type Id = WindowId;

    fn create(&mut self, spec: &WindowSpec) -> Result<WindowId> {
        let window = MacWindow::new(self.mtm, &spec.title, spec.size.width, spec.size.height);
        let id = self.next;
        self.next += 1;
        self.windows.insert(id, window);
        Ok(WindowId(id))
    }

    fn show(&mut self, id: WindowId) -> Result<()> {
        self.window(id)?.show();
        Ok(())
    }

    fn hide(&mut self, id: WindowId) -> Result<()> {
        self.window(id)?.hide();
        Ok(())
    }

    fn set_title(&mut self, id: WindowId, title: &str) -> Result<()> {
        self.window(id)?.set_title(title);
        Ok(())
    }

    fn set_size(&mut self, id: WindowId, size: LogicalSize) -> Result<()> {
        self.window(id)?.set_size(size.width, size.height);
        Ok(())
    }

    fn set_fullscreen(&mut self, id: WindowId, on: bool) -> Result<()> {
        let window = self.window(id)?;
        if window.is_fullscreen() != on {
            window.toggle_fullscreen();
        }
        Ok(())
    }

    fn close(&mut self, id: WindowId) -> Result<()> {
        let window = self
            .windows
            .remove(&id.0)
            .ok_or(Error::UnknownWindow(id.0 as u64))?;
        window.close();
        Ok(())
    }

    fn set_frameless(&mut self, id: WindowId, _on: bool) -> Result<()> {
        self.window(id)?;
        Err(Error::Unsupported {
            feature: "frameless (needs the style mask rebuilt without Titled)",
        })
    }
}

impl WebviewBackend for MacBackend {
    fn load_html(&mut self, id: WindowId, html: &str) -> Result<()> {
        self.window(id)?.load_html(html);
        Ok(())
    }

    fn navigate(&mut self, _id: WindowId, _url: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "navigate (needs the app:// scheme handler and navigation policy)",
        })
    }

    fn eval(&mut self, _id: WindowId, _script: &str) -> Result<()> {
        Err(Error::Unsupported { feature: "eval" })
    }

    fn post_message(&mut self, _id: WindowId, _message: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "post_message (needs the script message handler)",
        })
    }
}
