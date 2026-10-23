// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! tpt-shell: a webview app shell for Rust.
//!
//! This is the facade crate. [`App`] is the entry point: it gathers windows, grants and
//! commands, then starts them. Running today is Windows only; see [`app`].

#[cfg(feature = "agent")]
#[cfg_attr(not(windows), allow(dead_code))]
mod agent;
#[cfg(windows)]
mod devtools;
#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
#[cfg_attr(not(windows), allow(dead_code))]
mod endpoints;
#[cfg_attr(not(windows), allow(dead_code))]
mod dev_reload;
#[cfg_attr(not(windows), allow(dead_code))]
mod licenses;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod ui;

pub mod app;
#[cfg(windows)]
pub mod blob;

#[cfg(windows)]
pub use app::Running;
pub use app::{App, CommandError, Scope, Window};
pub use app::{MenuItem, TRAY_ACTIVATE};
/// The agent's tool type, for [`App::agent_tool`]. Needs the `agent` feature.
#[cfg(feature = "agent")]
pub use tpt_shell_agent::AgentTool;
pub use tpt_shell_audit::Policy;
pub use tpt_shell_capabilities::{ConsentPrompt, Engine, Grant};
pub use tpt_shell_core::{Error, Result};
/// Device support: the hub that opens devices under a grant, and the virtual devices.
pub use tpt_shell_devices as devices;
pub use tpt_shell_ipc::contract::Contract as CommandContract;
/// Argument schemas for [`App::agent_tool`]. Needs the `agent` feature.
#[cfg(feature = "agent")]
pub use tpt_shell_ipc::schema::Schema;
pub use tpt_shell_ipc::{Caller, Limits, Router, Target, Value};
pub use tpt_shell_macros::{
    command, commands, embed_dir, include_manifest, register_commands, Contract,
};
/// Where a window keeps its data: the default, a named profile, or an ephemeral session.
pub use tpt_shell_profiles::Kind as ProfileKind;
#[cfg(windows)]
pub use ui::UiHandle;

/// Paths the macros expand to. Not part of the API; app code uses the names above.
#[doc(hidden)]
pub mod __private {
    pub use tpt_shell_ipc as ipc;

    /// The asset that the app's asset source serves for `path` under the app origin, when the path
    /// is a stored blob (`__tpt/blob/<id>`). The same lookup the app uses, so the conformance
    /// harness can time the blob path that real blobs take. Not for apps.
    #[cfg(windows)]
    pub fn blob_asset_for_path(path: &str) -> Option<tpt_shell_core::backend::Asset> {
        path.strip_prefix("__tpt/blob/")
            .and_then(crate::blob::blob_asset)
    }
}
