// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Audit viewer: a window that checks an audit log's hash chain and shows its last entries.
//!
//! usage: audit-viewer <audit.jsonl>     (Set TPT_WEBVIEW2_LOADER to WebView2Loader.dll.)
//!
//! The page asks for the tail with `audit.tail`. The host verifies the whole chain first, so the
//! page shows "chain ok" only when the log is intact, and reports the first bad record otherwise.
//! The log is read whole for each request, which is fine for a viewer but not for a huge log.
//! Read-only: nothing here writes to the log.
//!
//! Windows only: the shell runs on WebView2 today.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tpt_shell::{embed_dir, App, CommandError, Error, Window};

const MAX_TAIL: usize = 200;

fn main() -> tpt_shell::Result<()> {
    let log = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without an audit log path",
        })?;

    App::new()
        .name("audit-viewer")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("Audit viewer").size(960.0, 640.0))
        .register(move |router| {
            // Read-only and unguarded: any page in the window may read the tail.
            router.register("audit.tail", move |args: &Value| tail(&log, args));
        })
        .run()
}

/// Verifies the chain, then returns the last `limit` entries, oldest first.
fn tail(log: &Path, args: &Value) -> Result<Value, CommandError> {
    let limit = args["limit"]
        .as_u64()
        .unwrap_or(50)
        .clamp(1, MAX_TAIL as u64) as usize;
    let summary = tpt_shell_audit::verify_file(log).map_err(|e| {
        CommandError::new("AUDIT_CHAIN_BROKEN", &format!("{e}"))
    })?;
    let text = std::fs::read_to_string(log)
        .map_err(|e| CommandError::new("AUDIT_READ", &e.to_string()))?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let entries: Vec<Value> = lines
        .iter()
        .rev()
        .take(limit)
        .rev()
        .map(|line| serde_json::from_str(line).unwrap_or(Value::Null))
        .collect();
    Ok(json!({
        "records": summary.records,
        "checkpoints": summary.checkpoints,
        "last_hash": summary.last_hash,
        "entries": entries,
    }))
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(2))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
