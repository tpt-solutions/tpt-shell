// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The `tpt-shell report` bug-report bundle (spec §10.5).
//!
//! A bundle is one JSON document with the replay, the logs, the versions and the tail of the
//! audit log. Every piece of text passes through a [`Redactor`] before it is written, so a
//! secret never leaves the machine unless the redactor lets it through. Replay assets are not
//! included: they can hold anything the page loaded, so only their names and sizes are listed.
//!
//! The default redactor removes the value of any object key that looks like a secret
//! (`password`, `token`, `secret`, `authorization`, `cookie`, `session`, `api_key`,
//! `credential`) and replaces any literal string the caller lists. Callers add their own
//! literals (for example the user's name, or the server host) through [`DefaultRedactor`].
//!
//! The CLI command `tpt-shell report` is the CLI agent's; it collects the inputs and calls
//! [`build`].
//!
//! Support: Full for the bundle and the default redactor. Partial: the redactor works on text
//! and on the keys of JSON objects; it cannot find a secret that is not written as a key or a
//! listed literal.

use serde_json::{json, Value};

use crate::{read, Mode, Recorder};

/// The most text kept from one log, in bytes. A longer log keeps its last part.
///
/// Support: Full.
pub const MAX_LOG_BYTES: usize = 1024 * 1024;

/// Replaces secrets.
///
/// Support: Full.
pub trait Redactor {
    /// Whether the value under this JSON object key must be removed.
    ///
    /// Support: Full.
    fn secret_key(&self, key: &str) -> bool;
    /// Returns the text with secrets removed.
    ///
    /// Support: Full.
    fn redact_text(&self, text: &str) -> String;
}

/// Key-based redaction plus a list of literal strings to hide.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultRedactor {
    /// Substrings of key names, lowercase. A key that contains one of these is a secret.
    pub key_fragments: Vec<String>,
    /// Literal strings to replace wherever they appear.
    pub literals: Vec<String>,
}

impl Default for DefaultRedactor {
    fn default() -> Self {
        DefaultRedactor {
            key_fragments: [
                "password",
                "passwd",
                "secret",
                "token",
                "authorization",
                "cookie",
                "session",
                "api_key",
                "apikey",
                "credential",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            literals: Vec::new(),
        }
    }
}

impl Redactor for DefaultRedactor {
    fn secret_key(&self, key: &str) -> bool {
        let lower = key.to_ascii_lowercase();
        self.key_fragments
            .iter()
            .any(|f| lower.contains(f.as_str()))
    }

    fn redact_text(&self, text: &str) -> String {
        let mut out = text.to_string();
        for literal in self.literals.iter().filter(|l| !l.is_empty()) {
            out = out.replace(literal.as_str(), "[redacted]");
        }
        out
    }
}

/// Redacts a JSON value in place: secret keys lose their value, and strings are passed
/// through the text redactor. Returns how many values were changed.
///
/// Support: Full.
pub fn redact_value(value: &mut Value, redactor: &dyn Redactor) -> usize {
    match value {
        Value::Object(map) => {
            let mut changed = 0;
            for (key, child) in map.iter_mut() {
                if redactor.secret_key(key) {
                    if *child != Value::String("[redacted]".into()) {
                        *child = Value::String("[redacted]".into());
                        changed += 1;
                    }
                } else {
                    changed += redact_value(child, redactor);
                }
            }
            changed
        }
        Value::Array(items) => items.iter_mut().map(|v| redact_value(v, redactor)).sum(),
        Value::String(s) => {
            let clean = redactor.redact_text(s);
            if clean != *s {
                *s = clean;
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Redacts a recording: its events are rewritten with redacted data, and its assets are
/// dropped.
///
/// Support: Full.
pub fn redact_replay(text: &str, redactor: &dyn Redactor) -> Result<String, String> {
    let replay = read(text, Mode::Lenient).map_err(|e| e.to_string())?;
    let mut recorder = Recorder::new(crate::Header {
        app: redactor.redact_text(&replay.header.app),
        platform: replay.header.platform.clone(),
    });
    for event in &replay.events {
        let mut data = event.data.clone();
        redact_value(&mut data, redactor);
        recorder.record(event.t_ns, &event.kind, data)?;
    }
    Ok(recorder.finish())
}

/// What goes into a bundle.
///
/// Support: Full.
#[derive(Clone, Debug, Default)]
pub struct Inputs<'a> {
    /// The recording, if one was made. It is read leniently, so a newer or damaged file still
    /// goes into the report.
    pub replay: Option<&'a str>,
    /// Named logs, such as `app.log`.
    pub logs: Vec<(String, String)>,
    /// Named versions, such as `tpt-shell` and `webview2`.
    pub versions: Vec<(String, String)>,
    /// The last lines of the audit log.
    pub audit_tail: Vec<String>,
}

/// Builds the bundle as JSON text.
///
/// Support: Full.
pub fn build(inputs: &Inputs, redactor: &dyn Redactor) -> Result<String, String> {
    let mut redactions = 0usize;
    let replay_text = match inputs.replay {
        Some(text) => Value::String(redact_replay(text, redactor)?),
        None => Value::Null,
    };
    let assets: Vec<Value> = match inputs.replay {
        Some(text) => read(text, Mode::Lenient)
            .map(|r| {
                r.assets
                    .iter()
                    .map(|a| json!({ "name": a.name, "bytes": a.bytes.len() }))
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    };

    let mut logs = serde_json::Map::new();
    for (name, text) in &inputs.logs {
        let kept = tail(text, MAX_LOG_BYTES);
        let clean = redactor.redact_text(kept);
        if clean != kept {
            redactions += 1;
        }
        logs.insert(name.clone(), Value::String(clean));
    }

    let mut audit = Vec::new();
    for line in &inputs.audit_tail {
        let clean = match serde_json::from_str::<Value>(line) {
            Ok(mut value) => {
                redactions += redact_value(&mut value, redactor);
                value.to_string()
            }
            Err(_) => {
                let clean = redactor.redact_text(line);
                if clean != *line {
                    redactions += 1;
                }
                clean
            }
        };
        audit.push(Value::String(clean));
    }

    let versions: serde_json::Map<String, Value> = inputs
        .versions
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();

    let bundle = json!({
        "format": "tpt-shell-report",
        "version": 1,
        "versions": versions,
        "replay": replay_text,
        "replay_assets": assets,
        "logs": logs,
        "audit_tail": audit,
        "redactions": redactions,
    });
    serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
}

/// The last `max` bytes of `text`, cut at a character boundary.
fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Header;

    fn recording_with_secret() -> String {
        let mut r = Recorder::new(Header {
            app: "demo".into(),
            platform: "windows".into(),
        });
        r.add_asset("cookie.bin", b"sessionid=abc").unwrap();
        r.record(
            0,
            "ipc_request",
            json!({ "id": 1, "cmd": "login", "args": { "password": "hunter2", "user": "ana" } }),
        )
        .unwrap();
        r.record(
            1,
            "input",
            json!({ "kind": "type", "text": "call 555-0101" }),
        )
        .unwrap();
        r.finish()
    }

    #[test]
    fn secret_keys_lose_their_values_in_the_replay() {
        let redacted =
            redact_replay(&recording_with_secret(), &DefaultRedactor::default()).unwrap();
        assert!(!redacted.contains("hunter2"));
        assert!(redacted.contains("\"user\":\"ana\""));
        let back = read(&redacted, Mode::Strict).unwrap();
        assert!(back.assets.is_empty(), "assets are dropped");
    }

    #[test]
    fn listed_literals_are_hidden_everywhere() {
        let redactor = DefaultRedactor {
            literals: vec!["555-0101".into()],
            ..DefaultRedactor::default()
        };
        let redacted = redact_replay(&recording_with_secret(), &redactor).unwrap();
        assert!(!redacted.contains("555-0101"));
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn the_bundle_contains_redacted_logs_and_audit_and_no_asset_bytes() {
        let inputs = Inputs {
            replay: Some(&recording_with_secret()),
            logs: vec![("app.log".into(), "token=abc123 opened main".into())],
            versions: vec![("tpt-shell".into(), "0.0.0".into())],
            audit_tail: vec![
                "{\"action\":\"login\",\"password\":\"hunter2\"}".into(),
                "plain line with abc123".into(),
            ],
        };
        let mut redactor = DefaultRedactor::default();
        redactor.literals.push("abc123".into());
        let text = build(&inputs, &redactor).unwrap();
        assert!(!text.contains("hunter2"));
        assert!(!text.contains("abc123"));
        assert!(
            !text.contains("c2Vzc2lvbmlk"),
            "asset bytes are not included"
        );
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["format"], "tpt-shell-report");
        assert_eq!(value["versions"]["tpt-shell"], "0.0.0");
        assert_eq!(value["replay_assets"][0]["name"], "cookie.bin");
        assert!(value["redactions"].as_u64().unwrap() >= 3);
    }

    #[test]
    fn a_damaged_replay_still_makes_a_report() {
        let text = format!("{}not json\n", recording_with_secret());
        let inputs = Inputs {
            replay: Some(&text),
            ..Inputs::default()
        };
        let report = build(&inputs, &DefaultRedactor::default()).unwrap();
        assert!(report.contains("\"replay\""));
    }

    #[test]
    fn a_log_keeps_its_tail_within_the_limit() {
        let long = "é".repeat(MAX_LOG_BYTES);
        let inputs = Inputs {
            logs: vec![("big.log".into(), long)],
            ..Inputs::default()
        };
        let text = build(&inputs, &DefaultRedactor::default()).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        let kept = value["logs"]["big.log"].as_str().unwrap();
        assert!(kept.len() <= MAX_LOG_BYTES);
        assert!(kept.chars().all(|c| c == 'é'));
    }

    #[test]
    fn key_matching_ignores_case() {
        let r = DefaultRedactor::default();
        assert!(r.secret_key("Authorization"));
        assert!(r.secret_key("X-API_KEY"));
        assert!(!r.secret_key("username"));
    }
}
