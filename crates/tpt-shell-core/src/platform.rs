// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Platform services that are not windows or webviews (spec §5.2, the `Platform` trait).
//! This is the M3 subset: clipboard text and the system light/dark setting. Menus, tray,
//! dialogs, notifications, deep links, single instance and the keystore come later.
//!
//! Every method has a default of [`Error::Unsupported`], so a backend lists what it provides.

use crate::error::{Error, Result};

/// The system's light or dark app setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

/// Plain-text clipboard.
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
pub trait ThemeBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        Err(Error::Unsupported { feature: "theme" })
    }
}
