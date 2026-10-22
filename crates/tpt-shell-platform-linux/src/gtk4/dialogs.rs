// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Open and save dialogs on GTK 4 (`GtkFileDialog`, GTK 4.10 and later).
//!
//! `GtkFileDialog` is asynchronous. The dialog is started and the main loop runs until the user
//! answers, as the GTK 3 adapter does with `gtk_dialog_run`. Filters are plain glob patterns
//! (see `route::valid_glob` in the GTK 3 module), so a filter cannot carry anything
//! but a name match. The dialog description is the GTK 3 adapter's [`FileDialog`], so callers
//! write the same code for either toolkit.
//!
//! On a GTK 4 older than 4.10 (Ubuntu 22.04 has 4.6) these calls return `Unsupported`; there is
//! no silent fallback. Under a desktop with the file chooser portal, the portal's dialog shows.
//!
//! Status: not run on a Linux machine. A dialog needs a person to answer it, so CI cannot run
//! one; only [`validate`] is tested. Partial: no preview widget, no folder-only or hidden-file
//! options.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::rc::Rc;

use tpt_shell_core::{Error, Result};

pub use crate::gtk3::dialogs::{validate, FileDialog, FileFilter};

use super::sys::{DialogFns, Gtk4};
use super::{cstring, from_c, pump, unsupported, Gtk4Backend, Obj, WindowId};

/// Which GTK call a dialog makes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Open,
    OpenMany,
    Save,
}

/// What the callback found. `None` until the dialog finishes; `Some(vec![])` when cancelled.
struct Outcome {
    gtk: &'static Gtk4,
    kind: Kind,
    paths: RefCell<Option<Vec<PathBuf>>>,
}

impl Gtk4Backend {
    /// Asks the user to pick one or more existing files. `None` when the dialog is cancelled.
    ///
    /// Support: Partial. Needs GTK 4.10 and WebKitGTK 6.0 at run time. Not run on macOS/Linux
    /// here; compile-checked.
    pub fn open_files(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<Vec<PathBuf>>> {
        let kind = if dialog.multiple {
            Kind::OpenMany
        } else {
            Kind::Open
        };
        self.run_file_dialog(parent, dialog, kind)
    }

    /// Asks the user where to save a file. `None` when the dialog is cancelled.
    ///
    /// Support: Partial. Needs GTK 4.10 and WebKitGTK 6.0 at run time. Not run on macOS/Linux
    /// here; compile-checked.
    pub fn save_file(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
    ) -> Result<Option<PathBuf>> {
        Ok(self
            .run_file_dialog(parent, dialog, Kind::Save)?
            .and_then(|mut paths| paths.pop()))
    }

    fn run_file_dialog(
        &mut self,
        parent: Option<WindowId>,
        dialog: &FileDialog,
        kind: Kind,
    ) -> Result<Option<Vec<PathBuf>>> {
        validate(dialog)?;
        let d = self.gtk.dialog_fns().map_err(|_| too_old_error())?;
        let parent_window = match parent {
            Some(id) => self.state(id)?.window,
            None => std::ptr::null_mut(),
        };
        let title = cstring(&dialog.title)?;
        let suggested = match (&dialog.suggested_name, kind) {
            (Some(name), Kind::Save) => Some(cstring(name)?),
            _ => None,
        };
        // Every string is made before the dialog starts, so no error can leave it half-built.
        let filters = dialog
            .filters
            .iter()
            .map(|filter| {
                let patterns = filter
                    .patterns
                    .iter()
                    .map(|p| cstring(p))
                    .collect::<Result<Vec<_>>>()?;
                Ok((cstring(&filter.name)?, patterns))
            })
            .collect::<Result<Vec<_>>>()?;
        let outcome = Rc::new(Outcome {
            gtk: self.gtk,
            kind,
            paths: RefCell::new(None),
        });
        let data = Rc::into_raw(Rc::clone(&outcome))
            .cast_mut()
            .cast::<c_void>();
        let f = &self.gtk.fns;
        // SAFETY: GTK is initialised and runs on this thread. The dialog and the filter store
        // are released before this function returns. `data` is a leaked `Rc` that
        // `dialog_done` takes back exactly once; the loop below waits for it, with no time limit,
        // because the user decides when to answer.
        unsafe {
            let win = (d.gtk_file_dialog_new)();
            (d.gtk_file_dialog_set_title)(win, title.as_ptr());
            (d.gtk_file_dialog_set_modal)(win, c_int_from(!parent_window.is_null()));
            if let Some(name) = &suggested {
                (d.gtk_file_dialog_set_initial_name)(win, name.as_ptr());
            }
            if !filters.is_empty() {
                let store = (d.g_list_store_new)((d.gtk_file_filter_get_type)());
                for (name, patterns) in &filters {
                    let ff = (d.gtk_file_filter_new)();
                    (d.gtk_file_filter_set_name)(ff, name.as_ptr());
                    for p in patterns {
                        (d.gtk_file_filter_add_pattern)(ff, p.as_ptr());
                    }
                    (d.g_list_store_append)(store, ff);
                    (f.g_object_unref)(ff);
                }
                (d.gtk_file_dialog_set_filters)(win, store);
                (f.g_object_unref)(store);
            }
            let callback = dialog_done as *const c_void;
            match kind {
                Kind::Open => (d.gtk_file_dialog_open)(
                    win,
                    parent_window,
                    std::ptr::null_mut(),
                    callback,
                    data,
                ),
                Kind::OpenMany => (d.gtk_file_dialog_open_multiple)(
                    win,
                    parent_window,
                    std::ptr::null_mut(),
                    callback,
                    data,
                ),
                Kind::Save => (d.gtk_file_dialog_save)(
                    win,
                    parent_window,
                    std::ptr::null_mut(),
                    callback,
                    data,
                ),
            }
            pump(self.gtk, || outcome.paths.borrow().is_some(), None);
            (f.g_object_unref)(win);
        }
        let paths = outcome.paths.borrow_mut().take().unwrap_or_default();
        Ok(if paths.is_empty() { None } else { Some(paths) })
    }
}

/// `gboolean` from a Rust bool.
fn c_int_from(value: bool) -> std::ffi::c_int {
    std::ffi::c_int::from(value)
}

/// GAsyncReadyCallback for the `GtkFileDialog` calls. A cancelled or failed dialog gives an
/// empty list.
///
/// # Safety
/// Called by GIO with the dialog, the async result, and the pointer from `run_file_dialog`.
unsafe extern "C" fn dialog_done(source: Obj, result: Obj, data: *mut c_void) {
    // SAFETY: `data` is the leaked `Rc<Outcome>` from `run_file_dialog`, used once.
    let outcome = unsafe { Rc::from_raw(data.cast::<Outcome>()) };
    let gtk = outcome.gtk;
    let Ok(d) = gtk.dialog_fns() else {
        *outcome.paths.borrow_mut() = Some(Vec::new());
        return;
    };
    let f = &gtk.fns;
    let mut error: Obj = std::ptr::null_mut();
    let mut paths = Vec::new();
    // SAFETY: the arguments are the ones GTK passed to this callback. Files and models returned
    // are owned by us and released after they are read; path strings are freed after copying.
    unsafe {
        match outcome.kind {
            Kind::Open | Kind::Save => {
                let file = if outcome.kind == Kind::Open {
                    (d.gtk_file_dialog_open_finish)(source, result, &mut error)
                } else {
                    (d.gtk_file_dialog_save_finish)(source, result, &mut error)
                };
                if !file.is_null() {
                    push_path(f, d, file, &mut paths);
                    (f.g_object_unref)(file);
                }
            }
            Kind::OpenMany => {
                let model = (d.gtk_file_dialog_open_multiple_finish)(source, result, &mut error);
                if !model.is_null() {
                    let count = (d.g_list_model_get_n_items)(model);
                    for i in 0..count {
                        let file = (d.g_list_model_get_item)(model, i);
                        if !file.is_null() {
                            push_path(f, d, file, &mut paths);
                            (f.g_object_unref)(file);
                        }
                    }
                    (f.g_object_unref)(model);
                }
            }
        }
        if !error.is_null() {
            (f.g_error_free)(error);
        }
    }
    *outcome.paths.borrow_mut() = Some(paths);
}

/// Appends the local path of a `GFile`, if it has one (a remote file may not).
///
/// # Safety
/// `file` is a live `GFile`.
unsafe fn push_path(f: &super::sys::Fns, d: &DialogFns, file: Obj, out: &mut Vec<PathBuf>) {
    // SAFETY: see the contract above; the string is owned by us and freed after copying.
    unsafe {
        let raw = (d.g_file_get_path)(file);
        if !raw.is_null() {
            out.push(PathBuf::from(from_c(raw)));
            (f.g_free)(raw.cast());
        }
    }
}

/// The error for a dialog that needs GTK 4.10 when the loaded GTK is older. Kept as a function so
/// tests can check the wording without GTK.
///
/// Support: Full.
pub fn too_old_error() -> Error {
    unsupported("file dialogs (need GTK 4.10 or later)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_description_is_validated_by_the_same_rules_as_gtk3() {
        let good = FileDialog {
            title: "Open".into(),
            multiple: true,
            filters: vec![FileFilter {
                name: "Text".into(),
                patterns: vec!["*.txt".into()],
            }],
            suggested_name: None,
        };
        assert!(validate(&good).is_ok());
        let bad = FileDialog {
            filters: vec![FileFilter {
                name: "x".into(),
                patterns: vec!["*.txt;rm".into()],
            }],
            ..FileDialog::default()
        };
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn an_old_gtk_is_reported_as_unsupported_not_as_success() {
        assert!(matches!(too_old_error(), Error::Unsupported { .. }));
    }
}
