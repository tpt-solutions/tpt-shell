// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Message boxes and file dialogs: `NSAlert`, `NSOpenPanel` and `NSSavePanel`. All run modal
//! on the main thread, so they take a [`MainThreadMarker`].

use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSString};
use tpt_shell_core::platform::{
    MessageAnswer, MessageBox, MessageButtons, MessageIcon, OpenDialog,
};

use crate::policy::file_extensions;

/// `NSModalResponseOK`: the user confirmed a panel.
const MODAL_OK: isize = 1;

/// The answers for each button, in the order the buttons are added (the first is the default).
fn button_answers(buttons: MessageButtons) -> &'static [(&'static str, MessageAnswer)] {
    match buttons {
        MessageButtons::Ok => &[("OK", MessageAnswer::Ok)],
        MessageButtons::OkCancel => &[("OK", MessageAnswer::Ok), ("Cancel", MessageAnswer::Cancel)],
        MessageButtons::YesNo => &[("Yes", MessageAnswer::Yes), ("No", MessageAnswer::No)],
        MessageButtons::YesNoCancel => &[
            ("Yes", MessageAnswer::Yes),
            ("No", MessageAnswer::No),
            ("Cancel", MessageAnswer::Cancel),
        ],
    }
}

/// Shows a modal alert and returns the button pressed. The alert is application modal: macOS
/// has no owner-window form that also blocks, so the owner is not used.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub fn message_box(mtm: MainThreadMarker, message: &MessageBox) -> MessageAnswer {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(&message.title));
    alert.setInformativeText(&NSString::from_str(&message.text));
    alert.setAlertStyle(match message.icon {
        MessageIcon::None | MessageIcon::Info => NSAlertStyle::Informational,
        MessageIcon::Warning => NSAlertStyle::Warning,
        MessageIcon::Error => NSAlertStyle::Critical,
    });
    let answers = button_answers(message.buttons);
    for (title, _) in answers {
        alert.addButtonWithTitle(&NSString::from_str(title));
    }
    let response = alert.runModal();
    let index = response - NSAlertFirstButtonReturn;
    usize::try_from(index)
        .ok()
        .and_then(|i| answers.get(i))
        .map(|(_, answer)| *answer)
        .unwrap_or(MessageAnswer::Cancel)
}

/// The array of extensions, or `None` for no restriction.
fn type_array(filters: &[(String, String)]) -> Option<Retained<NSArray<NSString>>> {
    let exts = file_extensions(filters);
    if exts.is_empty() {
        return None;
    }
    let strings: Vec<Retained<NSString>> = exts.iter().map(|e| NSString::from_str(e)).collect();
    Some(NSArray::from_retained_slice(&strings))
}

/// Shows an open-file dialog. An empty result means the user cancelled.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
#[allow(deprecated)] // allowedFileTypes: the UTType replacement needs another crate.
pub fn open_files(mtm: MainThreadMarker, dialog: &OpenDialog) -> Vec<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setTitle(Some(&NSString::from_str(&dialog.title)));
    panel.setAllowsMultipleSelection(dialog.multiple);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowedFileTypes(type_array(&dialog.filters).as_deref());
    if panel.runModal() != MODAL_OK {
        return Vec::new();
    }
    panel
        .URLs()
        .iter()
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}

/// Shows a save dialog with `suggested` as the file name. `None` means the user cancelled.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
#[allow(deprecated)] // allowedFileTypes: the UTType replacement needs another crate.
pub fn save_file(
    mtm: MainThreadMarker,
    title: &str,
    suggested: &str,
    filters: &[(String, String)],
) -> Option<PathBuf> {
    let panel = NSSavePanel::savePanel(mtm);
    panel.setTitle(Some(&NSString::from_str(title)));
    panel.setNameFieldStringValue(&NSString::from_str(suggested));
    panel.setAllowedFileTypes(type_array(filters).as_deref());
    if panel.runModal() != MODAL_OK {
        return None;
    }
    panel
        .URL()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
}
