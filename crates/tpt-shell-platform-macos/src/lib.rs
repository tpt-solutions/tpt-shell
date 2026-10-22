// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! macOS backend for tpt-shell (spec §6.2). The AppKit and WebKit code is built only on macOS.
//!
//! Status: windows (frameless, always-on-top, transparency, vibrancy backdrop, close action),
//! the WKWebView message bridge, document-start scripts, the navigation policy, the `app://`
//! scheme handler, snapshots, message boxes and file dialogs, the pasteboard text, appearance
//! observation, the menu bar tray, single instance by bundle id and the keychain are written
//! against the generated `objc2` bindings. The workspace type-checks them for the macOS target.
//! They have not run on a Mac yet; `tests/runtime.rs` runs them on macOS CI and skips with a
//! message when there is no GUI session. The policy rules in [`policy`] compile and are tested
//! on every host. The event loop, notifications, deep links and accessibility are not built.

/// Support: Full.
pub mod policy;

/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod backend;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod dialogs;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod keystore;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod shim;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod single_instance;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod theme;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod tray;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod webview;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
#[cfg(target_os = "macos")]
pub mod window;
