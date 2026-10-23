// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The developer overlay in the facade (Windows, debug builds only). Ctrl+Shift+D in an app page
//! shows the overlay window. Release builds never create it, and never add the key script to
//! the pages (`tpt_shell_devtools::enabled` is false there).
//!
//! The overlay window is created hidden when the app is built, and its page is loaded then. Each
//! time it is shown, the shell pushes a fresh snapshot into the page (`window.tptDevtools.update`).
//! The overlay's own page cannot call commands: its messages get a denied reply, because the
//! overlay holds no grants.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use serde_json::{json, Value};
use tpt_shell_core::backend::{
    CloseAction, MessageHandler, WebviewBackend, WindowBackend, WindowSpec,
};
use tpt_shell_core::geometry::LogicalSize;
use tpt_shell_core::Result;
use tpt_shell_devtools::{Capability, CommandInfo, Engine, Inspect, IpcEntry, Job, Snapshot};
use tpt_shell_ipc::contract::{json_schema, Contract, Kind};
use tpt_shell_platform_windows::backend::{eval_window, show_window, WindowId, WindowsBackend};

/// The message the key script posts. It is not a request, so it never reaches the router.
pub const HOTKEY_MESSAGE: &str = "tpt_devtools_hotkey";
/// The key the script listens for, with Ctrl and Shift.
pub const HOTKEY: char = 'd';

const WINDOW_NAME: &str = "tpt-devtools";
/// How many IPC calls the log keeps.
const IPC_LIMIT: usize = 200;

/// The shell state the overlay shows. The message handlers record into it, and the overlay reads
/// it through [`Inspect`].
///
/// Support: Partial. Jobs are not reported (the facade runs none). The capabilities are the
/// app's grants: a grant without consent is shown as in effect.
pub struct Shell {
    ipc: RefCell<VecDeque<IpcEntry>>,
    audit_path: Option<PathBuf>,
    capabilities: Vec<Capability>,
    contract: Vec<CommandInfo>,
}

impl Shell {
    pub fn new(
        audit_path: Option<PathBuf>,
        grants: &[tpt_shell_capabilities::Grant],
        contract: Option<&Contract>,
    ) -> Self {
        let mut capabilities: Vec<Capability> = grants
            .iter()
            .map(|g| Capability {
                name: format!("{} ({})", g.capability, g.window),
                granted: !g.consent,
            })
            .collect();
        capabilities.sort_by(|a, b| a.name.cmp(&b.name));
        let contract = contract
            .map(|c| {
                c.commands()
                    .map(|cmd| CommandInfo {
                        name: cmd.name.to_string(),
                        kind: match cmd.kind {
                            Kind::Sync => "sync",
                            Kind::Async => "async",
                            Kind::Job => "job",
                        }
                        .to_string(),
                        capability: cmd.capability.map(str::to_string),
                        args_schema: json_schema(&cmd.args),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Shell {
            ipc: RefCell::new(VecDeque::new()),
            audit_path,
            capabilities,
            contract,
        }
    }

    /// Records one IPC call for the log, dropping the oldest when the log is full.
    pub fn record(&self, entry: IpcEntry) {
        let mut log = self.ipc.borrow_mut();
        if log.len() == IPC_LIMIT {
            log.pop_front();
        }
        log.push_back(entry);
    }
}

impl Inspect for Shell {
    fn engine(&self) -> Engine {
        Engine {
            name: "webview2".into(),
            version: None,
            platform: "windows".into(),
        }
    }

    fn ipc_log(&self) -> Vec<IpcEntry> {
        self.ipc.borrow().iter().cloned().collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    fn jobs(&self) -> Vec<Job> {
        Vec::new()
    }

    fn audit_jsonl(&self, max: usize) -> String {
        let Some(path) = &self.audit_path else {
            return String::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(max)..].join("\n")
    }

    fn contract(&self) -> Vec<CommandInfo> {
        self.contract.clone()
    }
}

/// Creates the overlay window, hidden, with its page loaded. Returns `None` in a release build.
/// Its close hides it, so the app keeps running when the developer closes it.
///
/// Support: Partial. Debug builds on Windows only; not created in release builds.
pub fn create(backend: &mut WindowsBackend, shell: &Shell) -> Result<Option<WindowId>> {
    let Some(page) = tpt_shell_devtools::page_if_enabled(&Snapshot::collect(shell)) else {
        return Ok(None);
    };
    let id = backend.create(&WindowSpec {
        name: WINDOW_NAME.into(),
        title: "tpt-shell developer overlay".into(),
        size: LogicalSize {
            width: 900.0,
            height: 700.0,
        },
        allow_navigation: Vec::new(),
    })?;
    backend.set_close_action(id, CloseAction::Hide)?;
    let deny: MessageHandler = Box::new(|text| Some(denied_reply(text)));
    backend.set_message_handler(id, deny)?;
    backend.load_html(id, &page)?;
    Ok(Some(id))
}

/// Adds the Ctrl+Shift+D key script to an app window (debug builds only).
pub fn add_hotkey(backend: &mut WindowsBackend, id: WindowId) -> Result<()> {
    backend.page_key_message(id, HOTKEY, HOTKEY_MESSAGE)
}

/// Shows the overlay with the state as it is now.
pub fn open(overlay: WindowId, shell: &Shell) -> Result<()> {
    let json = Snapshot::collect(shell).to_json().to_string();
    show_window(overlay)?;
    eval_window(
        overlay,
        &format!("if (window.tptDevtools) {{ window.tptDevtools.update({json}); }}"),
    )
}

/// The reply to a call from the overlay page: denied, because the overlay has no grants.
fn denied_reply(text: &str) -> String {
    let id = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null);
    json!({
        "id": id,
        "ok": false,
        "error": {
            "code": "CAP_DENIED",
            "message": "the developer overlay holds no grants and cannot call commands",
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_from_the_overlay_is_denied_with_the_request_id() {
        let reply: Value =
            serde_json::from_str(&denied_reply(r#"{"v":1,"id":9,"cmd":"x"}"#)).expect("json");
        assert_eq!(reply["id"], json!(9));
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("CAP_DENIED"));
        let junk: Value = serde_json::from_str(&denied_reply("not json")).expect("json");
        assert_eq!(junk["id"], Value::Null);
    }

    #[test]
    fn the_ipc_log_keeps_the_newest_calls() {
        let shell = Shell::new(None, &[], None);
        for i in 0..(IPC_LIMIT as u64 + 5) {
            shell.record(IpcEntry {
                command: format!("c{i}"),
                latency_us: i,
                outcome: "ok".into(),
            });
        }
        let log = shell.ipc_log();
        assert_eq!(log.len(), IPC_LIMIT);
        assert_eq!(log[0].command, "c5");
        assert_eq!(log[IPC_LIMIT - 1].command, format!("c{}", IPC_LIMIT + 4));
    }

    #[test]
    fn the_audit_tail_is_the_last_lines_of_the_log() {
        let dir = std::env::temp_dir().join(format!("tpt-devtools-audit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("audit.jsonl");
        std::fs::write(&path, "one\ntwo\nthree\n").expect("write");
        let shell = Shell::new(Some(path), &[], None);
        assert_eq!(shell.audit_jsonl(2), "two\nthree");
        assert_eq!(shell.audit_jsonl(10), "one\ntwo\nthree");
        assert_eq!(Shell::new(None, &[], None).audit_jsonl(5), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
