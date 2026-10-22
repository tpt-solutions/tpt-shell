// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Window and webview operations of the macOS backend, behind the core traits.
//!
//! Every method that is not written yet returns [`Error::Unsupported`]. That keeps the gaps
//! visible to callers instead of silently doing nothing.

use std::collections::HashMap;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use tpt_shell_core::backend::{
    AssetSource, CloseAction, MessageHandler, WebviewBackend, WindowBackend, WindowSpec,
};
use tpt_shell_core::platform::{ClipboardBackend, ColorScheme, ThemeBackend};
use tpt_shell_core::{Error, LogicalSize, Result};

use crate::policy::is_app_url;
use crate::window::MacWindow;

/// Handle to a window managed by [`MacBackend`].
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowId(pub usize);

/// Owns the windows. Create it on the main thread.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct MacBackend {
    mtm: MainThreadMarker,
    next: usize,
    windows: HashMap<usize, MacWindow>,
}

impl MacBackend {
    /// `None` when the caller is not on the main thread.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
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
        let window = MacWindow::new(
            self.mtm,
            &spec.title,
            spec.size.width,
            spec.size.height,
            spec.allow_navigation.clone(),
        );
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

    fn set_frameless(&mut self, id: WindowId, on: bool) -> Result<()> {
        self.window(id)?.set_frameless(on);
        Ok(())
    }

    fn set_always_on_top(&mut self, id: WindowId, on: bool) -> Result<()> {
        self.window(id)?.set_always_on_top(on);
        Ok(())
    }

    fn set_transparent(&mut self, id: WindowId, on: bool) -> Result<()> {
        self.window(id)?.set_transparent(on);
        Ok(())
    }

    fn set_close_action(&mut self, id: WindowId, action: CloseAction) -> Result<()> {
        self.window(id)?.set_close_action(action);
        Ok(())
    }
}

impl WebviewBackend for MacBackend {
    fn load_html(&mut self, id: WindowId, html: &str) -> Result<()> {
        self.window(id)?.load_html(html);
        Ok(())
    }

    fn navigate(&mut self, id: WindowId, url: &str) -> Result<()> {
        if !is_app_url(url) {
            return Err(Error::Backend(format!("navigation blocked: {url}")));
        }
        self.window(id)?.navigate(url).map_err(Error::Backend)
    }

    /// Runs `script` in the page without waiting for it. Errors thrown by the script are not
    /// reported yet; only the call itself is.
    fn eval(&mut self, id: WindowId, script: &str) -> Result<()> {
        self.window(id)?.eval(script);
        Ok(())
    }

    fn post_message(&mut self, id: WindowId, message: &str) -> Result<()> {
        self.window(id)?.post_message(message);
        Ok(())
    }

    fn set_message_handler(&mut self, id: WindowId, handler: MessageHandler) -> Result<()> {
        self.window(id)?.set_message_handler(handler);
        Ok(())
    }

    fn set_asset_source(&mut self, id: WindowId, source: AssetSource) -> Result<()> {
        self.window(id)?.set_asset_source(source);
        Ok(())
    }
}

impl ClipboardBackend for MacBackend {
    fn clipboard_text(&self) -> Result<Option<String>> {
        let board = NSPasteboard::generalPasteboard();
        // SAFETY: NSPasteboardTypeString is a constant provided by AppKit.
        let text = unsafe { board.stringForType(NSPasteboardTypeString) };
        Ok(text.map(|s| s.to_string()))
    }

    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        // SAFETY: NSPasteboardTypeString is a constant provided by AppKit, and the string outlives the call.
        let ok =
            unsafe { board.setString_forType(&NSString::from_str(text), NSPasteboardTypeString) };
        if ok {
            Ok(())
        } else {
            Err(Error::Backend("the pasteboard refused the text".into()))
        }
    }
}

impl ThemeBackend for MacBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        let app = NSApplication::sharedApplication(self.mtm);
        let name = app.effectiveAppearance().name().to_string();
        // The dark appearances are NSAppearanceNameDarkAqua and the high contrast dark one.
        if name.contains("Dark") {
            Ok(ColorScheme::Dark)
        } else {
            Ok(ColorScheme::Light)
        }
    }
}
