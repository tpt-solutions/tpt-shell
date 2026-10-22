// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Window and webview operations of the macOS backend, behind the core traits.
//!
//! Every method that is not written yet returns [`Error::Unsupported`]. That keeps the gaps
//! visible to callers instead of silently doing nothing.

use std::collections::HashMap;
use std::path::PathBuf;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSBundle, NSString};
use tpt_shell_core::backend::{
    AssetSource, Backdrop, CloseAction, MessageHandler, PreferredScheme, WebviewBackend,
    WindowBackend, WindowSpec,
};
use tpt_shell_core::platform::{
    ClipboardBackend, ColorScheme, MessageAnswer, MessageBox, OpenDialog, Platform, ThemeBackend,
};
use tpt_shell_core::{Error, LogicalSize, Result};

use crate::keystore::Keystore;
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
    keystore: Keystore,
}

impl MacBackend {
    /// `None` when the caller is not on the main thread.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn new() -> Option<Self> {
        MainThreadMarker::new().map(|mtm| {
            // Keychain items are filed under the bundle id; a bare binary has none.
            let service = NSBundle::mainBundle()
                .bundleIdentifier()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "tpt-shell".to_string());
            MacBackend {
                mtm,
                next: 1,
                windows: HashMap::new(),
                keystore: Keystore::new(&service),
            }
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

    /// Mica, Acrylic and Tabbed all become a vibrancy material; see [`MacWindow::set_backdrop`].
    fn set_backdrop(&mut self, id: WindowId, backdrop: Backdrop) -> Result<()> {
        self.window(id)?.set_backdrop(self.mtm, backdrop);
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

    fn snapshot(&mut self, id: WindowId) -> Result<Vec<u8>> {
        self.window(id)?.snapshot(self.mtm).map_err(Error::Backend)
    }

    /// Web Inspector can be allowed, not opened: WebKit has no public call that shows it. The
    /// user opens it from the page's context menu. Needs macOS 13.3 or later.
    fn set_devtools(&mut self, id: WindowId, on: bool) -> Result<()> {
        tpt_shell_core::devtools_policy(!cfg!(debug_assertions), on)?;
        if self.window(id)?.set_inspectable(on) {
            Ok(())
        } else {
            Err(Error::Unsupported {
                feature: "developer tools before macOS 13.3",
            })
        }
    }

    fn open_devtools(&mut self, _id: WindowId) -> Result<()> {
        Err(Error::Unsupported {
            feature: "opening developer tools from code on macOS",
        })
    }

    fn set_preferred_color_scheme(&mut self, id: WindowId, scheme: PreferredScheme) -> Result<()> {
        self.window(id)?.set_preferred_scheme(scheme);
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

impl Platform for MacBackend {
    /// Application modal: `owner` is not used, because AppKit's owner-attached alert is a sheet
    /// that does not block.
    fn message_box(
        &mut self,
        _owner: Option<WindowId>,
        message: &MessageBox,
    ) -> Result<MessageAnswer> {
        Ok(crate::dialogs::message_box(self.mtm, message))
    }

    fn open_files(
        &mut self,
        _owner: Option<WindowId>,
        dialog: &OpenDialog,
    ) -> Result<Vec<PathBuf>> {
        Ok(crate::dialogs::open_files(self.mtm, dialog))
    }

    fn keystore_set(&mut self, name: &str, secret: &[u8]) -> Result<()> {
        self.keystore.set(name, secret)
    }

    fn keystore_get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.keystore.get(name)
    }

    fn keystore_delete(&mut self, name: &str) -> Result<bool> {
        self.keystore.delete(name)
    }
}

impl MacBackend {
    /// Shows a save dialog. `None` means the user cancelled.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn save_file(
        &mut self,
        title: &str,
        suggested: &str,
        filters: &[(String, String)],
    ) -> Option<PathBuf> {
        crate::dialogs::save_file(self.mtm, title, suggested, filters)
    }
}
