// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! tpt-shell: a webview app shell for Rust.
//!
//! This is the facade crate. [`App`] is the entry point: it gathers windows, grants and
//! commands, then starts them. Running today is Windows only; see [`app`].

pub mod app;
#[cfg(windows)]
pub mod blob;

#[cfg(windows)]
pub use app::Running;
pub use app::{App, CommandError, Scope, Window};
#[cfg(windows)]
pub use app::{MenuItem, TRAY_ACTIVATE};
pub use tpt_shell_capabilities::{ConsentPrompt, Engine, Grant};
pub use tpt_shell_core::{Error, Result};
/// Device support: the hub that opens devices under a grant, and the virtual devices.
pub use tpt_shell_devices as devices;
pub use tpt_shell_ipc::{Caller, Limits, Router, Target, Value};
pub use tpt_shell_macros::{command, commands, embed_dir, register_commands};
/// Where a window keeps its data: the default, a named profile, or an ephemeral session.
pub use tpt_shell_profiles::Kind as ProfileKind;

/// Paths the macros expand to. Not part of the API; app code uses the names above.
#[doc(hidden)]
pub mod __private {
    pub use tpt_shell_ipc as ipc;
}
