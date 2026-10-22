// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The native accessibility tree, and a comparison with the DOM (spec §10.6).
//!
//! The DOM tells us what the page promises. The native tree (UI Automation on Windows,
//! NSAccessibility on macOS, AT-SPI on Linux) is what a screen reader actually receives. This
//! module compares the two by role and name, so a control that the DOM names but the native
//! tree does not expose is reported.
//!
//! The platform dumps are implemented by the platform crates through [`NativeTree`]. This
//! crate only defines the trait and the comparison. Until a platform overrides `dump`, it
//! returns [`Error::Unsupported`].
//!
//! Support: Full for the comparison. Unsupported for the default dump; Windows (UIA), macOS
//! (NSAccessibility) and Linux (AT-SPI) overrides are routed to the platform agents.

use std::fmt::Write as _;

use tpt_shell_core::Error;

use crate::a11y::TreeNode;

/// Dumps the native accessibility tree of a window.
///
/// Support: Partial. The default dump returns Unsupported. The Windows, macOS and Linux overrides
/// are routed to the platform agents.
pub trait NativeTree {
    /// The nodes of the window's native tree, in document order.
    ///
    /// Support: Unsupported by default; a platform backend overrides this.
    fn dump(&self, window: &str) -> Result<Vec<TreeNode>, Error> {
        let _ = window;
        Err(Error::Unsupported {
            feature: "native accessibility tree dump",
        })
    }
}

/// A backend that has no native dump. It is what a platform without an override gets.
///
/// Support: Unsupported. The backend that every platform without an override gets. Its dump returns
/// Error::Unsupported.
pub struct NoNativeTree;

impl NativeTree for NoNativeTree {}

/// Collapses runs of whitespace and trims, so `Save  ` and `Save` are the same name.
fn normal_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A node that exists on one side only, or with a different role on the other side.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub name: String,
    pub dom_role: Option<String>,
    pub native_role: Option<String>,
}

/// The result of comparing the DOM with the native tree.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct DomNativeReport {
    /// Named nodes that match on role and name.
    pub matched: usize,
    /// Named nodes in the DOM with no native counterpart, so a screen reader cannot see them.
    pub only_dom: Vec<Mismatch>,
    /// Named nodes in the native tree with no DOM counterpart.
    pub only_native: Vec<Mismatch>,
    /// Same name on both sides, with a different role.
    pub role_differs: Vec<Mismatch>,
}

impl DomNativeReport {
    /// True when every named DOM node is exposed natively with the same role.
    ///
    /// Support: Full.
    pub fn passed(&self) -> bool {
        self.only_dom.is_empty() && self.only_native.is_empty() && self.role_differs.is_empty()
    }

    /// A plain-text report, one line per difference.
    ///
    /// Support: Full.
    pub fn render_text(&self) -> String {
        let mut out = format!("matched: {}\n", self.matched);
        for m in &self.only_dom {
            let _ = writeln!(
                out,
                "dom-only: {} ({})",
                m.name,
                m.dom_role.as_deref().unwrap_or("?")
            );
        }
        for m in &self.only_native {
            let _ = writeln!(
                out,
                "native-only: {} ({})",
                m.name,
                m.native_role.as_deref().unwrap_or("?")
            );
        }
        for m in &self.role_differs {
            let _ = writeln!(
                out,
                "role differs: {}: dom {} native {}",
                m.name,
                m.dom_role.as_deref().unwrap_or("?"),
                m.native_role.as_deref().unwrap_or("?")
            );
        }
        out
    }
}

/// Compares named nodes of the DOM and the native tree. Unnamed nodes (groups, containers)
/// are left out, because native trees add and merge them differently.
///
/// Matching is by role and name first. What is left is paired by name alone, so a role
/// difference is reported as one row, not as two missing nodes.
///
/// Support: Full.
pub fn compare_dom_native(dom: &[TreeNode], native: &[TreeNode]) -> DomNativeReport {
    let named = |nodes: &[TreeNode]| -> Vec<(String, String)> {
        nodes
            .iter()
            .filter_map(|n| {
                let name = normal_name(n.name.as_deref()?);
                (!name.is_empty()).then(|| (n.role.clone(), name))
            })
            .collect()
    };
    let mut dom_left = named(dom);
    let mut native_left = named(native);
    let mut report = DomNativeReport::default();

    // Pass 1: exact matches on role and name.
    let mut i = 0;
    while i < dom_left.len() {
        if let Some(j) = native_left.iter().position(|n| *n == dom_left[i]) {
            native_left.swap_remove(j);
            dom_left.swap_remove(i);
            report.matched += 1;
        } else {
            i += 1;
        }
    }

    // Pass 2: same name, different role.
    let mut i = 0;
    while i < dom_left.len() {
        let name = dom_left[i].1.clone();
        if let Some(j) = native_left.iter().position(|n| n.1 == name) {
            let (native_role, _) = native_left.swap_remove(j);
            let (dom_role, _) = dom_left.swap_remove(i);
            report.role_differs.push(Mismatch {
                name,
                dom_role: Some(dom_role),
                native_role: Some(native_role),
            });
        } else {
            i += 1;
        }
    }

    report.only_dom = dom_left
        .into_iter()
        .map(|(role, name)| Mismatch {
            name,
            dom_role: Some(role),
            native_role: None,
        })
        .collect();
    report.only_native = native_left
        .into_iter()
        .map(|(role, name)| Mismatch {
            name,
            dom_role: None,
            native_role: Some(role),
        })
        .collect();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(role: &str, name: Option<&str>) -> TreeNode {
        TreeNode {
            role: role.into(),
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn the_default_dump_is_unsupported() {
        let err = NoNativeTree.dump("main").unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
    }

    #[test]
    fn matching_trees_pass() {
        let dom = [node("button", Some("Compress")), node("group", None)];
        let native = [node("button", Some("Compress")), node("pane", None)];
        let report = compare_dom_native(&dom, &native);
        assert_eq!(report.matched, 1);
        assert!(report.passed());
    }

    #[test]
    fn a_control_missing_from_the_native_tree_is_reported() {
        let dom = [node("button", Some("Compress")), node("link", Some("Help"))];
        let native = [node("button", Some("Compress"))];
        let report = compare_dom_native(&dom, &native);
        assert_eq!(report.only_dom.len(), 1);
        assert_eq!(report.only_dom[0].name, "Help");
        assert!(!report.passed());
        assert!(report.render_text().contains("dom-only: Help (link)"));
    }

    #[test]
    fn a_role_difference_is_one_row_not_two() {
        let dom = [node("button", Some("Open"))];
        let native = [node("link", Some("Open"))];
        let report = compare_dom_native(&dom, &native);
        assert!(report.only_dom.is_empty());
        assert!(report.only_native.is_empty());
        assert_eq!(report.role_differs.len(), 1);
        assert_eq!(report.role_differs[0].native_role.as_deref(), Some("link"));
    }

    #[test]
    fn names_are_compared_after_whitespace_is_collapsed() {
        let dom = [node("button", Some("  Save   file "))];
        let native = [node("button", Some("Save file"))];
        assert!(compare_dom_native(&dom, &native).passed());
    }

    #[test]
    fn native_only_nodes_are_reported() {
        let report = compare_dom_native(&[], &[node("alert", Some("Saved"))]);
        assert_eq!(report.only_native.len(), 1);
        assert!(report.render_text().contains("native-only: Saved (alert)"));
    }
}
