// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The developer overlay (spec M3, M8, M9): a page that shows an app's IPC log with latency,
//! its jobs, the capabilities in effect, the audit tail, a contract explorer that can call
//! commands, and engine information.
//!
//! Design:
//! - The overlay reads shell state through the [`Inspect`] trait. The app implements it; this
//!   crate never reaches into the runtime itself.
//! - [`Snapshot`] is a plain, owned copy of that state. [`page`] turns it into one HTML file with
//!   no external resources, which the app opens in a window.
//! - The page shows the data it was given. The host refreshes it by calling
//!   `window.tptDevtools.update` with a new snapshot (`Snapshot::to_json`) each time the overlay
//!   is shown. The facade does this in `crates/tpt-shell/src/devtools.rs`.
//! - [`enabled`] is `true` only in debug builds. A release build gets `None` from [`page_if_enabled`]
//!   and never opens the overlay.
//!
//! Support: Full for the snapshot, the audit-line reader and the page, in debug builds on
//! Windows. Partial: the jobs panel stays empty in the facade (it runs no jobs), and the engine
//! version is not reported. Nothing here opens a socket or sends data anywhere; the page calls
//! only `window.tpt.invoke` when a developer presses Call.

#![forbid(unsafe_code)]

/// The overlay page. The placeholder is replaced by the snapshot as JSON.
const TEMPLATE: &str = include_str!("overlay.html");

/// The placeholder in the template that the snapshot replaces.
const DATA_PLACEHOLDER: &str = "__TPT_DEVTOOLS_DATA__";

/// Whether the overlay is available in this build. Only debug builds have it.
pub const fn enabled() -> bool {
    cfg!(debug_assertions)
}

/// The engine the app runs on and what it reports about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    /// For example `webview2`.
    pub name: String,
    /// Runtime version, when known.
    pub version: Option<String>,
    /// For example `windows`.
    pub platform: String,
}

/// One IPC call as the log shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpcEntry {
    pub command: String,
    /// Round trip time in microseconds.
    pub latency_us: u64,
    /// `ok`, or the error code the call failed with.
    pub outcome: String,
}

/// A capability and whether it is in effect for this app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    pub name: String,
    pub granted: bool,
}

/// A job the runtime is tracking.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u64,
    pub name: String,
    /// For example `running`, `done`, `cancelled`.
    pub state: String,
    /// Progress from 0.0 to 1.0, when the job reports it.
    pub progress: Option<f64>,
}

/// A command in the contract, as the explorer lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    pub name: String,
    /// For example `sync` or `async`.
    pub kind: String,
    /// The capability the command needs, if any.
    pub capability: Option<String>,
    /// The argument schema as JSON, so the explorer can show the fields.
    pub args_schema: serde_json::Value,
}

/// One line of the audit log, read leniently for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub seq: String,
    pub wall_unix_ms: String,
    pub actor: String,
    pub command: String,
    pub outcome: String,
}

/// The state the overlay shows. Each part is a copy taken when the snapshot was made.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub engine: Engine,
    pub ipc_log: Vec<IpcEntry>,
    pub capabilities: Vec<Capability>,
    pub jobs: Vec<Job>,
    pub audit_tail: Vec<AuditRow>,
    pub contract: Vec<CommandInfo>,
}

/// The shell state the overlay reads. The app implements this on whatever holds its runtime.
pub trait Inspect {
    fn engine(&self) -> Engine;
    fn ipc_log(&self) -> Vec<IpcEntry>;
    fn capabilities(&self) -> Vec<Capability>;
    fn jobs(&self) -> Vec<Job>;
    /// The last `max` lines of the audit log, as JSON Lines text.
    fn audit_jsonl(&self, max: usize) -> String;
    fn contract(&self) -> Vec<CommandInfo>;
}

/// How many audit rows the overlay keeps.
pub const AUDIT_TAIL: usize = 200;

impl Snapshot {
    /// Copies the state from `shell`. The audit tail is read from the JSON Lines text.
    pub fn collect(shell: &dyn Inspect) -> Snapshot {
        Snapshot {
            engine: shell.engine(),
            ipc_log: shell.ipc_log(),
            capabilities: shell.capabilities(),
            jobs: shell.jobs(),
            audit_tail: audit_rows(&shell.audit_jsonl(AUDIT_TAIL)),
            contract: shell.contract(),
        }
    }

    /// The snapshot as JSON. This is what the page reads.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "engine": {
                "name": self.engine.name,
                "version": self.engine.version,
                "platform": self.engine.platform,
            },
            "ipcLog": self.ipc_log.iter().map(|e| serde_json::json!({
                "command": e.command,
                "latencyUs": e.latency_us,
                "outcome": e.outcome,
            })).collect::<Vec<_>>(),
            "capabilities": self.capabilities.iter().map(|c| serde_json::json!({
                "name": c.name,
                "granted": c.granted,
            })).collect::<Vec<_>>(),
            "jobs": self.jobs.iter().map(|j| serde_json::json!({
                "id": j.id,
                "name": j.name,
                "state": j.state,
                "progress": j.progress,
            })).collect::<Vec<_>>(),
            "auditTail": self.audit_tail.iter().map(|a| serde_json::json!({
                "seq": a.seq,
                "wallUnixMs": a.wall_unix_ms,
                "actor": a.actor,
                "command": a.command,
                "outcome": a.outcome,
            })).collect::<Vec<_>>(),
            "contract": self.contract.iter().map(|c| serde_json::json!({
                "name": c.name,
                "kind": c.kind,
                "capability": c.capability,
                "args": c.args_schema,
            })).collect::<Vec<_>>(),
        })
    }
}

/// Reads audit lines for display. A line that is not a JSON object is skipped, so a damaged
/// log still shows its good rows. The log is verified elsewhere (`tpt-shell audit verify`).
pub fn audit_rows(jsonl: &str) -> Vec<AuditRow> {
    jsonl
        .lines()
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).ok()?;
            if !record.is_object() {
                return None;
            }
            let field = |name: &str| match &record[name] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            Some(AuditRow {
                seq: field("seq"),
                wall_unix_ms: field("wall_unix_ms"),
                actor: field("actor"),
                command: field("command"),
                outcome: field("outcome"),
            })
        })
        .collect()
}

/// Makes JSON safe to place inside a `<script>` element: no `</` can end the element early.
fn script_safe(json: &str) -> String {
    json.replace("</", "<\\/")
}

/// The overlay page for `snapshot`. It has no external resources.
pub fn page(snapshot: &Snapshot) -> String {
    let data = script_safe(&snapshot.to_json().to_string());
    TEMPLATE.replacen(DATA_PLACEHOLDER, &data, 1)
}

/// The overlay page in a debug build, `None` in a release build.
pub fn page_if_enabled(snapshot: &Snapshot) -> Option<String> {
    enabled().then(|| page(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl Inspect for Fake {
        fn engine(&self) -> Engine {
            Engine {
                name: "webview2".into(),
                version: Some("155.0".into()),
                platform: "windows".into(),
            }
        }
        fn ipc_log(&self) -> Vec<IpcEntry> {
            vec![IpcEntry {
                command: "greet".into(),
                latency_us: 420,
                outcome: "ok".into(),
            }]
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability {
                name: "fs.read".into(),
                granted: false,
            }]
        }
        fn jobs(&self) -> Vec<Job> {
            vec![Job {
                id: 7,
                name: "scan".into(),
                state: "running".into(),
                progress: Some(0.5),
            }]
        }
        fn audit_jsonl(&self, max: usize) -> String {
            let line = "{\"seq\":0,\"wall_unix_ms\":1700000000000,\"actor\":\"page\",\"command\":\"greet\",\"outcome\":\"ok\"}";
            let all = format!("{line}\nnot json\n{line}\n");
            all.lines().rev().take(max).collect::<Vec<_>>().join("\n")
        }
        fn contract(&self) -> Vec<CommandInfo> {
            vec![CommandInfo {
                name: "greet".into(),
                kind: "sync".into(),
                capability: None,
                args_schema: serde_json::json!({ "type": "struct" }),
            }]
        }
    }

    #[test]
    fn a_snapshot_copies_every_part_of_the_shell() {
        let snapshot = Snapshot::collect(&Fake);
        assert_eq!(snapshot.engine.name, "webview2");
        assert_eq!(snapshot.ipc_log[0].latency_us, 420);
        assert!(!snapshot.capabilities[0].granted);
        assert_eq!(snapshot.jobs[0].progress, Some(0.5));
        assert_eq!(snapshot.contract[0].name, "greet");
    }

    #[test]
    fn damaged_audit_lines_are_skipped_not_fatal() {
        let rows = audit_rows("{\"seq\":1,\"actor\":\"page\"}\nnot json\n[1,2]\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].seq, "1");
        assert_eq!(rows[0].wall_unix_ms, "");
    }

    #[test]
    fn the_page_embeds_the_data_and_has_no_external_resources() {
        let html = page(&Snapshot::collect(&Fake));
        assert!(!html.contains(DATA_PLACEHOLDER));
        assert!(html.contains("\"latencyUs\":420"));
        assert!(!html.contains("<script src"));
        assert!(!html.contains("<link "));
        assert!(!html.contains("http://") && !html.contains("https://"));
    }

    #[test]
    fn data_cannot_close_the_script_element() {
        let clean = page(&Snapshot::collect(&Fake)).matches("</script>").count();
        let mut snapshot = Snapshot::collect(&Fake);
        snapshot.ipc_log[0].command = "</script><script>alert(1)</script>".into();
        let html = page(&snapshot);
        assert_eq!(
            html.matches("</script>").count(),
            clean,
            "data added a close tag"
        );
        assert!(html.contains("<\\/script>"));
    }

    #[test]
    fn the_page_has_all_six_panels_and_a_call_button() {
        let html = page(&Snapshot::collect(&Fake));
        for panel in ["ipc", "jobs", "caps", "audit", "contract", "engine"] {
            assert!(
                html.contains(&format!("data-panel=\"{panel}\"")),
                "missing panel {panel}"
            );
        }
        assert!(html.contains("el(\"button\", \"Call\""));
        assert!(html.contains("window.tpt.invoke(name, parsed)"));
    }

    #[test]
    fn the_explorer_fills_arguments_from_the_schema_properties() {
        // The contract writes a command's arguments as a JSON Schema struct, with the fields
        // under `properties`. The explorer's sample must read that shape.
        let html = page(&Snapshot::collect(&Fake));
        assert!(html.contains("schema.properties"));
        assert!(!html.contains("schema.fields"));
    }

    #[test]
    fn the_page_refresh_hook_is_exposed_for_the_host() {
        let html = page(&Snapshot::collect(&Fake));
        assert!(html.contains("window.tptDevtools = {"));
        assert!(html.contains("update: function (next)"));
    }

    #[test]
    fn the_overlay_is_off_in_release_builds() {
        let snapshot = Snapshot::collect(&Fake);
        assert_eq!(page_if_enabled(&snapshot).is_some(), enabled());
        assert_eq!(enabled(), cfg!(debug_assertions));
    }
}
