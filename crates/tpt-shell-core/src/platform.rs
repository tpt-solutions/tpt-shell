// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Platform services that are not windows or webviews (spec §5.2, the `Platform` trait).
//! Clipboard text, the system light/dark setting, message boxes, the open-files dialog, the
//! keystore, deep-link registration and notifications. Menus, tray and single instance are not
//! in the trait yet: the Windows tray and single instance are separate modules of that crate.
//!
//! Every method has a default of [`Error::Unsupported`], so a backend lists what it provides.

use std::path::PathBuf;

use crate::backend::WindowBackend;
use crate::error::{Error, Result};

/// The system's light or dark app setting.
///
/// Support: Full. A plain enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

/// Plain-text clipboard.
///
/// Support: Partial. Plain text only. Windows and the fake are verified; The macOS and Linux code
/// for it has not run on a real system.
pub trait ClipboardBackend {
    /// The clipboard's text, or `None` when it holds no text.
    fn clipboard_text(&self) -> Result<Option<String>> {
        Err(Error::Unsupported {
            feature: "clipboard",
        })
    }

    /// Replaces the clipboard's contents with `text`.
    fn set_clipboard_text(&mut self, _text: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "clipboard",
        })
    }
}

/// System appearance.
///
/// Support: Partial. Windows reads the system setting. The macOS and Linux code for it has not run
/// on a real system.
pub trait ThemeBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        Err(Error::Unsupported { feature: "theme" })
    }
}

/// Which buttons a message box offers.
///
/// Support: Full. A plain enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageButtons {
    #[default]
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

/// The icon a message box shows.
///
/// Support: Full. A plain enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageIcon {
    #[default]
    None,
    Info,
    Warning,
    Error,
}

/// The button the user pressed. A closed box is `Cancel`.
///
/// Support: Full. A plain enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageAnswer {
    Ok,
    Cancel,
    Yes,
    No,
}

/// A modal message box.
///
/// Support: Full. Plain data; the Windows backend shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageBox {
    pub title: String,
    pub text: String,
    pub buttons: MessageButtons,
    pub icon: MessageIcon,
}

/// An open-file dialog. `filters` are (display name, pattern) pairs such as ("Text", "*.txt");
/// empty means all files.
///
/// Support: Full. Plain data; the Windows backend shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenDialog {
    pub title: String,
    pub filters: Vec<(String, String)>,
    /// Whether the user may pick several files.
    pub multiple: bool,
}

/// Services a platform offers the app. Implemented by each backend next to its window and
/// webview traits, so a dialog can be owned by a window.
///
/// Support: Partial. Windows is verified for message boxes, the open dialog, the keystore and deep
/// links. Notifications go through Running::notify on Windows, not this trait. macOS code has not
/// run, and Linux implements none of this trait.
pub trait Platform: WindowBackend + ClipboardBackend + ThemeBackend {
    /// Shows a modal message box owned by `owner`, or by no window, and returns the button.
    fn message_box(
        &mut self,
        _owner: Option<Self::Id>,
        _message: &MessageBox,
    ) -> Result<MessageAnswer> {
        Err(Error::Unsupported {
            feature: "message box",
        })
    }

    /// Shows an open-file dialog. Returns the chosen paths; an empty list means cancelled.
    fn open_files(
        &mut self,
        _owner: Option<Self::Id>,
        _dialog: &OpenDialog,
    ) -> Result<Vec<PathBuf>> {
        Err(Error::Unsupported {
            feature: "open dialog",
        })
    }

    /// Stores a secret under `name`, protected for this user.
    fn keystore_set(&mut self, _name: &str, _secret: &[u8]) -> Result<()> {
        Err(Error::Unsupported {
            feature: "keystore",
        })
    }

    /// The secret stored under `name`, or `None`.
    fn keystore_get(&self, _name: &str) -> Result<Option<Vec<u8>>> {
        Err(Error::Unsupported {
            feature: "keystore",
        })
    }

    /// Removes the secret stored under `name`. Returns whether there was one.
    fn keystore_delete(&mut self, _name: &str) -> Result<bool> {
        Err(Error::Unsupported {
            feature: "keystore",
        })
    }

    /// Registers a URL scheme (such as `myapp:`) for this app, so links open it.
    fn register_scheme(&mut self, _scheme: &str, _description: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "deep links",
        })
    }

    /// Removes a URL scheme registered by [`Platform::register_scheme`].
    fn unregister_scheme(&mut self, _scheme: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "deep links",
        })
    }

    /// Shows a notification with `title` and `body`.
    fn notify(&mut self, _title: &str, _body: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "notifications",
        })
    }
}
