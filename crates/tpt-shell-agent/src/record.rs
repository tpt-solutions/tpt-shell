// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Recording of agent actions (spec §10.3, §10.4). Every call the session handles, refused or
//! run, goes to a [`Recorder`]. [`AuditRecorder`] writes it to the hash-chained audit file, and
//! the record's parameters follow the audit file's redaction policy. The replay crate (§10.5)
//! implements [`Recorder`] too; that wiring belongs to the test crate.

use std::time::{Duration, Instant};

use serde_json::Value;
use tpt_shell_audit::{AuditError, AuditLog, Entry, Outcome as AuditOutcome};

use crate::Outcome;

/// Writing an action failed. The session stops, because an action that cannot be recorded
/// must not run. Fix: check the audit file's volume and permissions.
///
/// Support: Full.
pub const CODE_RECORD: &str = "TS0113";

/// One handled call, as a recorder sees it.
///
/// Support: Full.
pub struct RecordedAction<'a> {
    /// Who acted, such as `agent:assistant`.
    pub actor: &'a str,
    pub command: &'a str,
    pub args: &'a Value,
    pub outcome: &'a Outcome,
    /// How long the call took, measured by the session.
    pub duration: Duration,
    /// Wall-clock time of the call, in milliseconds since the Unix epoch.
    pub wall_unix_ms: u64,
}

/// Receives every handled call. An error stops the session.
///
/// Support: Full.
pub trait Recorder {
    fn record(&mut self, action: &RecordedAction) -> Result<(), String>;
}

/// Writes each action to an audit log.
///
/// Support: Full.
pub struct AuditRecorder {
    log: AuditLog,
    started: Instant,
}

impl AuditRecorder {
    /// Support: Full.
    pub fn new(log: AuditLog) -> Self {
        AuditRecorder {
            log,
            started: Instant::now(),
        }
    }

    /// Gives the log back, for example to verify it when the session ends.
    ///
    /// Support: Full.
    pub fn into_log(self) -> AuditLog {
        self.log
    }
}

impl Recorder for AuditRecorder {
    fn record(&mut self, action: &RecordedAction) -> Result<(), String> {
        let outcome = match action.outcome {
            Outcome::Ran => AuditOutcome::Ok,
            Outcome::Failed(_) => AuditOutcome::Err("AGENT_FAILED".into()),
            Outcome::Refused(_) => AuditOutcome::Err("AGENT_REFUSED".into()),
        };
        let entry = Entry {
            wall_unix_ms: action.wall_unix_ms,
            mono_ns: u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            actor: action.actor.to_string(),
            command: action.command.to_string(),
            capability: None,
            params: action.args.clone(),
            outcome,
            duration_us: u64::try_from(action.duration.as_micros()).unwrap_or(u64::MAX),
        };
        self.log
            .append(&entry)
            .map(|_| ())
            .map_err(|e: AuditError| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tpt_shell_audit::Policy;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tpt-agent-record-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn every_outcome_is_written_and_passwords_are_redacted() {
        let path = temp_path("actions.jsonl");
        let policy = Policy {
            store_params: true,
            redact_keys: vec!["password".into()],
        };
        let log = AuditLog::open(&path, policy).unwrap();
        let mut recorder = AuditRecorder::new(log);
        let args = json!({ "password": "hunter2", "user": "ada" });
        for outcome in [
            Outcome::Ran,
            Outcome::Failed("boom".into()),
            Outcome::Refused("no".into()),
        ] {
            recorder
                .record(&RecordedAction {
                    actor: "agent:assistant",
                    command: "login",
                    args: &args,
                    outcome: &outcome,
                    duration: Duration::from_micros(5),
                    wall_unix_ms: 1,
                })
                .unwrap();
        }
        drop(recorder);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(!text.contains("hunter2"));
        assert!(text.contains("AGENT_REFUSED"));
        assert!(text.contains("agent:assistant"));
        assert!(tpt_shell_audit::verify_file(&path).is_ok());
    }
}
