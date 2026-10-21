// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! tpt-shell: a webview app shell for Rust.
//!
//! This is the facade crate. [`App`] is the entry point: it gathers windows, grants and
//! commands, then starts them. Running today is Windows only; see [`app`].

pub mod app;

#[cfg(windows)]
pub use app::Running;
pub use app::{App, CommandError, Scope, Window};
pub use tpt_shell_capabilities::Grant;
pub use tpt_shell_core::{Error, Result};
pub use tpt_shell_ipc::{Caller, Target, Value};
