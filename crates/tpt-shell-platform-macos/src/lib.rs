// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! macOS backend for tpt-shell (spec §6.2). The AppKit and WebKit code is built only on macOS.
//!
//! Status: windows (frameless, always-on-top, transparency, close action), the WKWebView
//! message bridge, the navigation policy, the `app://` scheme handler and page scripts are
//! written against the generated `objc2` bindings. The workspace can type-check them for the
//! macOS target. They have not run on a Mac yet. The policy rules in [`policy`] compile and
//! are tested on every host. The event loop, dialogs, pasteboard, notifications, tray, deep
//! links, single instance, keystore and accessibility are not built.

/// Support: Full.
pub mod policy;

/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod backend;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod shim;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod webview;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod window;
