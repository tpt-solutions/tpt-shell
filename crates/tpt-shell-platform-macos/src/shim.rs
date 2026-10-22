// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The page-side runtime (spec §7.2), embedded from `js/runtime/`. The backend injects it at
//! document start, after the bridge, so `window.tpt.invoke` exists before any page script runs.

/// Source of `js/runtime/tpt-shell.js`.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub const SHIM_JS: &str = include_str!("../../../js/runtime/tpt-shell.js");
