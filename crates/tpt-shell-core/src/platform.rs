// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Platform services that are not windows or webviews (spec §5.2, the `Platform` trait).
//! Clipboard text, the system light/dark setting and accent colour, message boxes, the open-files
//! dialog, the keystore, deep-link registration, the notification-area icon and notifications.
//!
//! Not in the trait yet: application menus (not built on any platform) and single instance. Single
//! instance is claimed once, when the app starts, and it returns a guard rather than acting per
//! call, so it stays a start-up step of each backend's crate.
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

/// An opaque colour, 8 bits per channel.
///
/// Support: Full. A plain struct.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// System appearance.
///
/// Support: Partial. Windows reads the system setting and the accent colour. The macOS and Linux
/// code for it has not run on a real system.
pub trait ThemeBackend {
    fn color_scheme(&self) -> Result<ColorScheme> {
        Err(Error::Unsupported { feature: "theme" })
    }

    /// The system accent colour (the colour of the Windows accent, or the macOS highlight).
    fn accent_color(&self) -> Result<Rgb> {
        Err(Error::Unsupported {
            feature: "accent colour",
        })
    }
}

/// One item in a notification-area icon's context menu. Ids must be non-zero: zero is the id a
/// left click on the icon reports.
///
/// Support: Full. A plain struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayItem {
    pub id: u32,
    pub label: String,
}

/// The id a left click on a tray icon reports.
///
/// Support: Full.
pub const TRAY_ACTIVATE: u32 = 0;

/// One entry in a menu: a command that reports its id when picked, or a separator line.
///
/// Support: Full. A plain enum. Command ids must be non-zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Command {
        id: u32,
        label: String,
        enabled: bool,
    },
    Separator,
}

/// A top-level menu in a window's menu bar, such as "File".
///
/// Support: Full. Plain data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub entries: Vec<MenuEntry>,
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
/// Support: Partial. Windows is verified for message boxes, the open dialog, the keystore, deep
/// links, the tray icon and notifications. macOS code has not run, and Linux implements none of
/// this trait.
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

    /// Shows an icon in the notification area with `tooltip` and a context menu. A left click
    /// calls `on_select` with [`TRAY_ACTIVATE`]; a menu pick calls it with that item's id. An icon
    /// already shown is replaced. The callback runs on the UI thread.
    fn set_tray(
        &mut self,
        _tooltip: &str,
        _items: &[TrayItem],
        _on_select: Box<dyn FnMut(u32)>,
    ) -> Result<()> {
        Err(Error::Unsupported {
            feature: "tray icon",
        })
    }

    /// Removes the icon shown by [`Platform::set_tray`]. Nothing happens when there is none.
    fn remove_tray(&mut self) -> Result<()> {
        Err(Error::Unsupported {
            feature: "tray icon",
        })
    }

    /// Sets the menu bar of every top-level window to `menus`, and calls `on_command` with the id
    /// of a command when the user picks it. An empty `menus` removes the menu bar. Windows created
    /// later get the same menu bar. Replaces the menu bar set before.
    fn set_menu_bar(&mut self, _menus: &[Menu], _on_command: Box<dyn FnMut(u32)>) -> Result<()> {
        Err(Error::Unsupported {
            feature: "menu bar",
        })
    }

    /// Shows a notification with `title` and `body`. On Windows this is a balloon from the tray
    /// icon, so it needs one: a call with no icon shown is an error.
    fn notify(&mut self, _title: &str, _body: &str) -> Result<()> {
        Err(Error::Unsupported {
            feature: "notifications",
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::fake::FakeBackend;
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn the_tray_icon_reports_clicks_and_menu_picks_to_its_callback_until_removed() {
        let mut b = FakeBackend::new();
        let picks = Rc::new(RefCell::new(Vec::new()));
        let seen = picks.clone();
        let items = [TrayItem {
            id: 7,
            label: "Open".into(),
        }];
        b.set_tray(
            "TPT",
            &items,
            Box::new(move |id| seen.borrow_mut().push(id)),
        )
        .expect("set_tray");
        assert_eq!(b.tray.as_ref().map(|t| t.0.as_str()), Some("TPT"));
        assert!(b.click_tray(TRAY_ACTIVATE));
        assert!(b.click_tray(7));
        assert_eq!(*picks.borrow(), vec![0, 7]);
        b.remove_tray().expect("remove");
        assert!(b.tray.is_none());
        assert!(!b.click_tray(7), "no icon, no callback");
    }

    #[test]
    fn a_zero_menu_id_is_refused_because_zero_is_the_left_click() {
        let mut b = FakeBackend::new();
        let bad = [TrayItem {
            id: 0,
            label: "x".into(),
        }];
        assert!(b.set_tray("t", &bad, Box::new(|_| {})).is_err());
        assert!(b.tray.is_none());
    }

    #[test]
    fn a_menu_bar_reports_picks_of_enabled_commands_and_zero_ids_are_refused() {
        let mut b = FakeBackend::new();
        let file = Menu {
            title: "File".into(),
            entries: vec![
                MenuEntry::Command {
                    id: 10,
                    label: "Open".into(),
                    enabled: true,
                },
                MenuEntry::Separator,
                MenuEntry::Command {
                    id: 11,
                    label: "Locked".into(),
                    enabled: false,
                },
            ],
        };
        let picks = Rc::new(RefCell::new(Vec::new()));
        let seen = picks.clone();
        b.set_menu_bar(
            std::slice::from_ref(&file),
            Box::new(move |id| seen.borrow_mut().push(id)),
        )
        .expect("set_menu_bar");
        assert!(b.pick_menu(10));
        assert!(!b.pick_menu(11), "a disabled command cannot be picked");
        assert!(!b.pick_menu(99), "an unknown command is not picked");
        assert_eq!(*picks.borrow(), vec![10]);
        let zero = Menu {
            title: "Bad".into(),
            entries: vec![MenuEntry::Command {
                id: 0,
                label: "x".into(),
                enabled: true,
            }],
        };
        assert!(b.set_menu_bar(&[zero], Box::new(|_| {})).is_err());
        b.set_menu_bar(&[], Box::new(|_| {})).expect("remove");
        assert!(b.menu_bar.is_empty());
        assert!(!b.pick_menu(10), "no menu bar, no pick");
    }

    #[test]
    fn a_notification_needs_a_tray_icon_and_the_accent_comes_from_the_backend() {
        let mut b = FakeBackend::new();
        assert!(b.notify("hi", "there").is_err(), "no icon, no balloon");
        b.set_tray("t", &[], Box::new(|_| {})).expect("set_tray");
        b.notify("hi", "there").expect("notify");
        assert_eq!(b.notifications, vec![("hi".into(), "there".into())]);
        b.accent = Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(b.accent_color().expect("accent").g, 2);
    }
}
