// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Windows native accessibility tree for the DOM comparison (spec §10.6).
//!
//! [`crate::uia::dump`] returns UI Automation rows. This module turns them into the test crate's
//! [`TreeNode`]s, so [`tpt_shell_test::native::compare_dom_native`] can compare them with the
//! DOM. [`WindowsNativeTree`] is the [`NativeTree`] for the windows a test has named.
//!
//! Dependency direction: this crate depends on `tpt-shell-test`, and `tpt-shell-test` depends only
//! on the core and replay crates, so there is no cycle.

use tpt_shell_core::Error;
use tpt_shell_test::a11y::TreeNode;
use tpt_shell_test::native::NativeTree;
use windows_sys::Win32::Foundation::HWND;

use crate::backend::WindowId;
use crate::uia::{self, UiaNode};

/// Converts one UI Automation row to a tree node. The role and the name are kept. The enabled flag
/// and the bounds are not: a [`TreeNode`] has no fields for them, and the comparison reads only the
/// role and the name.
///
/// Support: Full. Unit tested.
pub fn tree_node(row: &UiaNode) -> TreeNode {
    TreeNode {
        role: row.role.to_string(),
        name: row.name.clone(),
    }
}

/// The native tree of the windows a test has named, read through UI Automation.
///
/// The caller names each window, because the comparison asks for a window by name. A name that was
/// not added is an error, not an empty tree.
///
/// Support: Partial. Windows only. Each window is read through UI Automation by its handle, and the
/// enabled flag and bounds are dropped by the conversion (see [`tree_node`]).
#[derive(Clone, Debug, Default)]
pub struct WindowsNativeTree {
    windows: Vec<(String, WindowId)>,
}

impl WindowsNativeTree {
    /// An empty set of named windows.
    ///
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Names a window, so [`NativeTree::dump`] can find it by `name`.
    ///
    /// Support: Full.
    pub fn add(&mut self, name: impl Into<String>, window: WindowId) -> &mut Self {
        self.windows.push((name.into(), window));
        self
    }
}

impl NativeTree for WindowsNativeTree {
    fn dump(&self, window: &str) -> Result<Vec<TreeNode>, Error> {
        let Some((_, id)) = self.windows.iter().find(|(name, _)| name == window) else {
            return Err(Error::Backend(format!(
                "no window named {window:?} for the native tree"
            )));
        };
        let rows = uia::dump(id.0 as HWND)?;
        Ok(rows.iter().map(tree_node).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null_mut;
    use tpt_shell_test::native::compare_dom_native;

    fn row(role: &'static str, name: Option<&str>, enabled: bool) -> UiaNode {
        UiaNode {
            role,
            control_type: 50000,
            name: name.map(str::to_string),
            enabled,
            bounds: None,
        }
    }

    #[test]
    fn a_row_keeps_its_role_and_name() {
        let node = tree_node(&row("button", Some("Save"), true));
        assert_eq!(
            node,
            TreeNode {
                role: "button".into(),
                name: Some("Save".into()),
            }
        );
    }

    #[test]
    fn an_empty_name_stays_none() {
        let node = tree_node(&row("group", None, true));
        assert_eq!(node.name, None);
    }

    #[test]
    fn a_disabled_row_is_kept_with_the_same_node_as_when_enabled() {
        // The enabled flag is not carried, so a disabled control is still compared by its role and
        // name, and the enabled state cannot change the result.
        let enabled = tree_node(&row("button", Some("Save"), true));
        let disabled = tree_node(&row("button", Some("Save"), false));
        assert_eq!(enabled, disabled);
    }

    #[test]
    fn bounds_do_not_change_the_node() {
        let mut with_bounds = row("button", Some("Save"), true);
        with_bounds.bounds = Some(crate::uia::Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        });
        assert_eq!(
            tree_node(&with_bounds),
            tree_node(&row("button", Some("Save"), true))
        );
    }

    #[test]
    fn a_window_that_was_not_named_is_an_error() {
        let tree = WindowsNativeTree::new();
        let err = tree.dump("main").unwrap_err();
        assert!(
            err.to_string().contains("no window named"),
            "unexpected error: {err}"
        );
    }

    /// Creates a popup with a child button "Save", compares a DOM fixture of the same window with
    /// its native tree, and reports the result. Skips when there is no desktop to create the window
    /// on. The window's own node is in the fixture, because the comparison reports a native node
    /// with no DOM counterpart.
    #[test]
    fn a_dom_fixture_of_a_popup_matches_its_native_tree() {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_CHILD,
            WS_POPUP, WS_VISIBLE,
        };

        // A real window class, so UI Automation reports the root as a window. A STATIC control
        // would be reported as text. A class that is already registered returns an error, which
        // is fine: the first registration in the process is the one used.
        let class_name = crate::com::wide("TptNativeTreeTestWindow");
        let instance = unsafe { GetModuleHandleW(null_mut()) };
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        // SAFETY: the class name and the procedure live for the process; the result is only used to
        // tell a fresh class from one registered earlier in this process.
        unsafe { RegisterClassW(&wc) };

        let class = class_name;
        let button_class = crate::com::wide("BUTTON");
        let title = crate::com::wide("Native tree test window");
        let label = crate::com::wide("Save");
        // SAFETY: the strings are NUL-terminated and live for each call. A null return means no
        // window could be created, which is the no-desktop case.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                100,
                100,
                240,
                120,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if window.is_null() {
            eprintln!("skipped: no desktop to create a window on");
            return;
        }
        // SAFETY: the parent is the window created above, and both are destroyed exactly once,
        // below.
        let child = unsafe {
            CreateWindowExW(
                0,
                button_class.as_ptr(),
                label.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                10,
                10,
                80,
                24,
                window,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        assert!(!child.is_null(), "the button could not be created");

        let mut tree = WindowsNativeTree::new();
        tree.add("popup", WindowId(window as isize));
        let native = tree.dump("popup");
        // SAFETY: the child and the window are ours; the child is destroyed before its parent.
        unsafe {
            DestroyWindow(child);
            DestroyWindow(window);
        }
        let native = native.expect("UI Automation dumps the popup");
        // The first row is the window itself. UI Automation reports a plain top-level window as a
        // pane with the title as its name. That is window chrome, not page content, and a page's DOM
        // does not contain it, so the comparison is made on the content below the root.
        assert!(
            native.len() >= 2,
            "the dump has the window and the button: {native:?}"
        );
        let content = &native[1..];

        let dom = vec![TreeNode {
            role: "button".into(),
            name: Some("Save".into()),
        }];
        let report = compare_dom_native(&dom, content);
        eprintln!("native tree comparison:\n{}", report.render_text());
        assert!(
            report.passed(),
            "the popup's native tree does not match its DOM fixture:\n{}",
            report.render_text()
        );
    }
}
