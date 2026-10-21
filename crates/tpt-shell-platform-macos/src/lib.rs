// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! macOS backend for tpt-shell (spec §6.2). Built only on macOS; empty elsewhere.
//!
//! Status: window lifecycle and HTML loading are written against the generated `objc2`
//! bindings. The workspace can type-check them for the macOS target. Nothing here has run on
//! a Mac yet. Navigation policy, the script bridge, the event loop and the other platform
//! services are not built.

#[cfg(target_os = "macos")]
pub mod backend;
#[cfg(target_os = "macos")]
pub mod window;
