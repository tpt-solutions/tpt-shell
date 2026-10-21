// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Open and save dialogs on GTK 3 (`GtkFileChooserWidget` inside a `GtkDialog`).
//!
//! The dialog is built from non-variadic calls (the variadic `gtk_file_chooser_dialog_new` is
//! not used), and it runs a nested main loop until the user answers. Filters are plain glob
//! patterns (see [`route::valid_glob`]), so a filter cannot carry anything but a name match.
//!
//! Status: not run on a Linux machine. Partial: no preview widget, no extra file-chooser
//! options (hidden files, places, folders-only), and a dialog is always modal to its parent.

use std::ffi::c_void;
use std::path::PathBuf;

use tpt_shell_core::{Error, Result};

use super::route;
use super::sys::{
    self, GSList, GTK_FILE_CHOOSER_ACTION_OPEN, GTK_FILE_CHOOSER_ACTION_SAVE, GTK_RESPONSE_ACCEPT,
    GTK_RESPONSE_CANCEL,
};
use super::{cstring, from_c, Gtk3Backend, WindowId};

/// The longest name a filter may carry.
const MAX_FILTER_NAME: usize = 64;

/// One filter in the dialog's type list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    pub name: String,
    /// Glob patterns such as `*.txt`. Validated by [`route::valid_glob`].
    pub patterns: Vec<String>,
}

/// What the dialog shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDialog {
    pub title: String,
    /// Allow picking several files. Ignored by the save dialog.
    pub multiple: bool,
    pub filters: Vec<FileFilter>,
    /// The name a save dialog starts with.
    pub suggested_name: Option<String>,
}

/// Checks a dialog before any GTK call. Refuses a bad filter name or pattern, or a name with a
/// NUL byte, so nothing unsafe reaches the toolkit.
pub fn validate(dialog: &FileDialog) -> Result<()> {
    if dialog.title.contains('\0') {
        return Err(Error::Backend("the dialog title holds a NUL byte".into()));
    }
    for filter in &dialog.filters {
        if filter.name.is_empty()
            || filter.name.len() > MAX_FILTER_NAME
            || filter.name.contains('\0')
        {
            return Err(Error::Backend(
                "a filter name must be 1-64 bytes with no NUL".into(),
            ));
        }
        if filter.patterns.is_empty() {
            return Err(Error::Backend("a filter needs at least one pattern".into()));
        }
        if let Some(bad) = filter.patterns.iter().find(|p| !route::valid_glob(p)) {
            return Err(Error::Backend(format!(
                "`{bad}` is not a plain file-name pattern"
            )));
        }
    }
    if let Some(name) = &dialog.suggested_name {
        if name.contains('\0') || name.contains('/') {
            return Err(Error::Backend(
                "a suggested file name holds a slash or a NUL".into(),
            ));
        }
    }
    Ok(())
}

impl Gtk3Backend {
    /// Asks the user to pick one or more existing files. `None` when the dialog is cancelled.
    pub fn open_files(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<Vec<PathBuf>>> {
        self.run_file_dialog(parent, dialog, GTK_FILE_CHOOSER_ACTION_OPEN)
    }

    /// Asks the user where to save a file. `None` when the dialog is cancelled.
    pub fn save_file(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<PathBuf>> {
        let mut dialog = dialog.clone();
        dialog.multiple = false;
        Ok(self
            .run_file_dialog(parent, &dialog, GTK_FILE_CHOOSER_ACTION_SAVE)?
            .and_then(|mut paths| paths.pop()))
    }

    fn run_file_dialog(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
        action: i32,
    ) -> Result<Option<Vec<PathBuf>>> {
        validate(dialog)?;
        let parent_window = match parent {
            Some(id) => Some(self.state(id)?.window),
            None => None,
        };
        let title = cstring(&dialog.title)?;
        let save = action == GTK_FILE_CHOOSER_ACTION_SAVE;
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised and runs on this thread. Every widget made here is either
        // handed to its parent or destroyed before this function returns. The chooser's file
        // list is read before the dialog is destroyed.
        unsafe {
            let win = (f.gtk_dialog_new)();
            (f.gtk_window_set_title)(win, title.as_ptr());
            if let Some(p) = parent_window {
                (f.gtk_window_set_transient_for)(win, p);
                (f.gtk_window_set_modal)(win, 1);
            }
            let chooser = (f.gtk_file_chooser_widget_new)(action);
            if dialog.multiple && !save {
                (f.gtk_file_chooser_set_select_multiple)(chooser, 1);
            }
            if save {
                (f.gtk_file_chooser_set_do_overwrite_confirmation)(chooser, 1);
                if let Some(name) = &dialog.suggested_name {
                    let name = cstring(name)?;
                    (f.gtk_file_chooser_set_current_name)(chooser, name.as_ptr());
                }
            }
            for filter in &dialog.filters {
                let ff = (f.gtk_file_filter_new)();
                let name = cstring(&filter.name)?;
                (f.gtk_file_filter_set_name)(ff, name.as_ptr());
                for pattern in &filter.patterns {
                    let p = cstring(pattern)?;
                    (f.gtk_file_filter_add_pattern)(ff, p.as_ptr());
                }
                (f.gtk_file_chooser_add_filter)(chooser, ff);
            }
            let content = (f.gtk_dialog_get_content_area)(win);
            (f.gtk_container_add)(content, chooser);
            let cancel = c"Cancel";
            let accept = if save { c"Save" } else { c"Open" };
            (f.gtk_dialog_add_button)(win, cancel.as_ptr(), GTK_RESPONSE_CANCEL);
            (f.gtk_dialog_add_button)(win, accept.as_ptr(), GTK_RESPONSE_ACCEPT);
            (f.gtk_widget_show_all)(win);
            let response = (f.gtk_dialog_run)(win);
            let paths = if response == GTK_RESPONSE_ACCEPT {
                Some(collect(self.gtk, chooser, dialog.multiple && !save))
            } else {
                None
            };
            (f.gtk_widget_destroy)(win);
            Ok(paths)
        }
    }
}

/// Reads the chosen file names from the chooser, and frees what GTK allocated for them.
///
/// # Safety
/// `chooser` is a live `GtkFileChooser`, and the dialog that holds it has not been destroyed.
unsafe fn collect(gtk: &sys::Gtk3, chooser: *mut c_void, many: bool) -> Vec<PathBuf> {
    let f = &gtk.fns;
    let mut out = Vec::new();
    // SAFETY: see the contract above. GTK returns owned strings and a list; both are freed.
    unsafe {
        if many {
            let list: *mut GSList = (f.gtk_file_chooser_get_filenames)(chooser);
            let mut node = list;
            while !node.is_null() {
                let name = (*node).data.cast::<std::ffi::c_char>();
                if !name.is_null() {
                    out.push(PathBuf::from(from_c(name)));
                    (f.g_free)(name.cast());
                }
                node = (*node).next;
            }
            if !list.is_null() {
                (f.g_slist_free)(list);
            }
        } else {
            let name = (f.gtk_file_chooser_get_filename)(chooser);
            if !name.is_null() {
                out.push(PathBuf::from(from_c(name)));
                (f.g_free)(name.cast());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(name: &str, patterns: &[&str]) -> FileFilter {
        FileFilter {
            name: name.to_string(),
            patterns: patterns.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn a_plain_dialog_is_valid() {
        let dialog = FileDialog {
            title: "Open".into(),
            multiple: true,
            filters: vec![filter("Text", &["*.txt"])],
            suggested_name: Some("notes.txt".into()),
        };
        assert!(validate(&dialog).is_ok());
    }

    #[test]
    fn bad_filters_and_names_are_refused_before_gtk() {
        let bad_pattern = FileDialog {
            filters: vec![filter("Text", &["*.txt;rm -rf"])],
            ..FileDialog::default()
        };
        assert!(validate(&bad_pattern).is_err());
        let empty_patterns = FileDialog {
            filters: vec![filter("Text", &[])],
            ..FileDialog::default()
        };
        assert!(validate(&empty_patterns).is_err());
        let nul_title = FileDialog {
            title: "a\0b".into(),
            ..FileDialog::default()
        };
        assert!(validate(&nul_title).is_err());
        let slash_name = FileDialog {
            suggested_name: Some("../escape".into()),
            ..FileDialog::default()
        };
        assert!(validate(&slash_name).is_err());
    }
}
