// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell replay` and `tpt-shell report`, over `tpt_shell_replay`.
//!
//! `replay` reads a `.tptreplay` file (strictly by default, or leniently) and plays it with the
//! [`Player`](tpt_shell_replay::player). The only target the library has without a window is the
//! echo reference, so the play checks the recording against that reference. Running the app
//! itself needs its platform window, which is not routed yet, and the command says so.
//!
//! `report` writes the bug-report bundle: the recording (redacted), named logs, versions and the
//! tail of an audit log. Secrets are removed by the default redactor, plus any text given with
//! `--redact`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt_shell_replay::player::{self, Echo, PlayError};
use tpt_shell_replay::report::{self, DefaultRedactor, Inputs};
use tpt_shell_replay::Mode;

/// How many trailing audit lines a report keeps.
const AUDIT_TAIL_LINES: usize = 50;

const REPLAY_USAGE: &str = "usage: tpt-shell replay <file.tptreplay> [--strict | --lenient]";

const REPORT_USAGE: &str = "usage: tpt-shell report <out.json> [--replay <file>] [--log <file>]...
                            [--audit <audit.jsonl>] [--version <name>=<value>]...
                            [--redact <text>]...";

/// Plays a recording. Exit 0 when it plays with no divergence (or lenient mode reports one), 1
/// when the recording is unreadable or strict mode finds a difference.
///
/// Support: Full.
pub fn replay(args: &[&str]) -> Result<ExitCode, String> {
    let mut path = None;
    let mut mode = Mode::Strict;
    for arg in args {
        match *arg {
            "--strict" => mode = Mode::Strict,
            "--lenient" => mode = Mode::Lenient,
            value if path.is_none() && !value.starts_with("--") => path = Some(value),
            _ => return Err(REPLAY_USAGE.into()),
        }
    }
    let path = path.ok_or_else(|| REPLAY_USAGE.to_string())?;
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;

    let recording = match tpt_shell_replay::read(&text, mode) {
        Ok(recording) => recording,
        Err(e) => {
            println!("{path}: {e}");
            return Ok(ExitCode::from(1));
        }
    };
    println!(
        "{path}: format v{}, app {}, platform {}, {} event(s), {} asset(s)",
        recording.version,
        recording.header.app,
        recording.header.platform,
        recording.events.len(),
        recording.assets.len()
    );
    if recording.skipped > 0 {
        println!("skipped {} unknown or malformed line(s)", recording.skipped);
    }

    let mut target = Echo;
    let code = match player::play(&recording, &mut target, mode) {
        Ok(outcome) => match outcome.divergence {
            None => {
                println!("played with no divergence");
                ExitCode::SUCCESS
            }
            Some(d) => {
                println!("diverged at event {}: {}", d.index, d.reason);
                ExitCode::SUCCESS
            }
        },
        Err(PlayError::Diverged(d)) => {
            println!("replay diverged at event {}: {}", d.index, d.reason);
            ExitCode::from(1)
        }
        Err(error @ PlayError::Target { .. }) => {
            println!("{error}");
            ExitCode::from(1)
        }
    };
    println!(
        "note: this checked the recording against the echo reference target. Running the app \
         itself needs its platform window, which is not routed yet."
    );
    Ok(code)
}

/// The parsed arguments of `report`.
#[derive(Debug, Default, PartialEq)]
struct ReportArgs {
    out: String,
    replay: Option<String>,
    logs: Vec<String>,
    audit: Option<String>,
    versions: Vec<(String, String)>,
    redact: Vec<String>,
}

fn parse_report_args(args: &[&str]) -> Result<ReportArgs, String> {
    let mut out = None;
    let mut parsed = ReportArgs::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--replay" | "--log" | "--audit" | "--version" | "--redact" => {
                let value = *rest
                    .next()
                    .ok_or_else(|| format!("{arg} needs a value\n{REPORT_USAGE}"))?;
                match *arg {
                    "--replay" => parsed.replay = Some(value.to_string()),
                    "--log" => parsed.logs.push(value.to_string()),
                    "--audit" => parsed.audit = Some(value.to_string()),
                    "--redact" => parsed.redact.push(value.to_string()),
                    _ => {
                        let (name, version) = value
                            .split_once('=')
                            .filter(|(n, _)| !n.is_empty())
                            .ok_or_else(|| format!("--version needs name=value, not {value}"))?;
                        parsed
                            .versions
                            .push((name.to_string(), version.to_string()));
                    }
                }
            }
            value if out.is_none() && !value.starts_with("--") => out = Some(value.to_string()),
            _ => return Err(REPORT_USAGE.into()),
        }
    }
    parsed.out = out.ok_or_else(|| REPORT_USAGE.to_string())?;
    Ok(parsed)
}

/// Writes the bug-report bundle to the file named first on the command line.
///
/// Support: Full.
pub fn report(args: &[&str]) -> Result<ExitCode, String> {
    let parsed = parse_report_args(args)?;
    let replay_text = match &parsed.replay {
        Some(path) => {
            Some(fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?)
        }
        None => None,
    };
    let mut logs = Vec::new();
    for path in &parsed.logs {
        let bytes = fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
        logs.push((
            file_name(Path::new(path)),
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    let audit_tail = match &parsed.audit {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(AUDIT_TAIL_LINES);
            lines[start..].iter().map(|l| l.to_string()).collect()
        }
        None => Vec::new(),
    };
    let mut versions = vec![(
        "tpt-shell".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )];
    versions.extend(parsed.versions.iter().cloned());

    let inputs = Inputs {
        replay: replay_text.as_deref(),
        logs,
        versions,
        audit_tail,
    };
    let redactor = DefaultRedactor {
        literals: parsed.redact.clone(),
        ..DefaultRedactor::default()
    };
    let bundle = report::build(&inputs, &redactor)?;
    let out = PathBuf::from(&parsed.out);
    fs::write(&out, format!("{bundle}\n"))
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!("wrote {}", out.display());
    Ok(ExitCode::SUCCESS)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tpt_shell_replay::{Header, Recorder};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("tpt-cli-replay-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn header() -> Header {
        Header {
            app: "demo".into(),
            platform: "linux".into(),
        }
    }

    /// A recording whose responses match what the echo reference produces.
    fn consistent_recording() -> String {
        let mut r = Recorder::new(header());
        r.record(
            0,
            "ipc_request",
            serde_json::json!({ "id": 1, "cmd": "greet" }),
        )
        .unwrap();
        r.record(
            1,
            "ipc_response",
            serde_json::json!({ "id": 1, "ok": true, "data": "greet" }),
        )
        .unwrap();
        r.finish()
    }

    #[test]
    fn a_consistent_recording_plays_clean() {
        let dir = scratch("clean");
        let path = dir.join("run.tptreplay");
        fs::write(&path, consistent_recording()).unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(replay(&[path]).unwrap(), ExitCode::SUCCESS);
        assert_eq!(replay(&[path, "--lenient"]).unwrap(), ExitCode::SUCCESS);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_that_disagrees_with_the_echo_fails_in_strict_mode() {
        let dir = scratch("diverge");
        let mut r = Recorder::new(header());
        r.record(
            0,
            "ipc_request",
            serde_json::json!({ "id": 1, "cmd": "greet" }),
        )
        .unwrap();
        r.record(
            1,
            "ipc_response",
            serde_json::json!({ "id": 1, "ok": true, "data": "something else" }),
        )
        .unwrap();
        let path = dir.join("run.tptreplay");
        fs::write(&path, r.finish()).unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(replay(&[path]).unwrap(), ExitCode::from(1));
        assert_eq!(replay(&[path, "--lenient"]).unwrap(), ExitCode::SUCCESS);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_recording_is_a_problem_not_an_io_failure() {
        let dir = scratch("broken");
        let path = dir.join("bad.tptreplay");
        fs::write(&path, "this is not a recording\n").unwrap();
        assert_eq!(
            replay(&[path.to_str().unwrap()]).unwrap(),
            ExitCode::from(1)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replay_usage_errors_are_refused() {
        assert!(replay(&[]).is_err());
        assert!(replay(&["a.tptreplay", "--fast"]).is_err());
        assert!(replay(&["no-such-file.tptreplay"]).is_err());
    }

    #[test]
    fn report_parses_its_flags_and_needs_an_output() {
        let parsed = parse_report_args(&[
            "out.json",
            "--replay",
            "r.tptreplay",
            "--log",
            "app.log",
            "--log",
            "other.log",
            "--audit",
            "audit.jsonl",
            "--version",
            "webview2=128.0",
            "--redact",
            "alice",
        ])
        .unwrap();
        assert_eq!(parsed.out, "out.json");
        assert_eq!(parsed.logs, vec!["app.log", "other.log"]);
        assert_eq!(parsed.versions, vec![("webview2".into(), "128.0".into())]);
        assert_eq!(parsed.redact, vec!["alice"]);
        assert!(parse_report_args(&["--replay", "r"]).is_err());
        assert!(parse_report_args(&["out.json", "--version", "noequals"]).is_err());
        assert!(parse_report_args(&["out.json", "--log"]).is_err());
        assert!(parse_report_args(&["out.json", "extra.json"]).is_err());
    }

    #[test]
    fn report_removes_secrets_and_keeps_the_rest() {
        let dir = scratch("report");
        let mut r = Recorder::new(header());
        r.record(
            0,
            "ipc_request",
            serde_json::json!({ "id": 1, "cmd": "login", "password": "hunter2" }),
        )
        .unwrap();
        let replay_path = dir.join("run.tptreplay");
        fs::write(&replay_path, r.finish()).unwrap();
        let log_path = dir.join("app.log");
        fs::write(&log_path, "started for alice\n").unwrap();
        let audit_path = dir.join("audit.jsonl");
        fs::write(&audit_path, "{\"seq\":0}\n").unwrap();
        let out = dir.join("bundle.json");

        let args = [
            out.to_str().unwrap(),
            "--replay",
            replay_path.to_str().unwrap(),
            "--log",
            log_path.to_str().unwrap(),
            "--audit",
            audit_path.to_str().unwrap(),
            "--version",
            "webview2=128.0",
            "--redact",
            "alice",
        ];
        assert_eq!(report(&args).unwrap(), ExitCode::SUCCESS);
        let bundle = fs::read_to_string(&out).unwrap();
        assert!(!bundle.contains("hunter2"), "a password reached the bundle");
        assert!(!bundle.contains("alice"), "a literal reached the bundle");
        assert!(bundle.contains("app.log"));
        assert!(bundle.contains("webview2"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_reports_a_missing_input_as_an_error() {
        let dir = scratch("missing");
        let out = dir.join("bundle.json");
        assert!(report(&[out.to_str().unwrap(), "--log", "no-such.log"]).is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
