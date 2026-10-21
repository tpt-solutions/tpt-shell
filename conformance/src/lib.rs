// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Backend conformance suite (spec §14).
//!
//! The same pages and scripted checks run on every backend. `tests/conformance.rs` runs them on
//! the current OS and writes one result file. The `conformance-report` binary merges the result
//! files of all OSes into the compatibility report.
//!
//! Outcomes:
//! - `Pass` and `Fail` come from a check that ran.
//! - `Pending` means the backend does not provide the feature yet, or the check is written but
//!   has no pass criterion yet. The reason says which.
//! - `Skipped` means the check could not run in this session, such as no WebView2 loader.
//!
//! Only `Pass` counts as green.

use serde_json::{json, Value};
use std::fmt::Write as _;

/// Every check, in report order. The names are the ones listed in todo.md (M4, conformance).
pub const CHECKS: &[&str] = &[
    "ipc-round-trip",
    "large-blobs",
    "streams-backpressure",
    "custom-protocol",
    "csp",
    "navigation-policy",
    "theme-injection",
    "snapshot",
    "drag-regions",
    "dpi",
];

/// What one check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail(String),
    Pending(String),
    Skipped(String),
}

impl Outcome {
    /// The upper-case label used in the markdown report.
    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::Fail(_) => "FAIL",
            Outcome::Pending(_) => "PENDING",
            Outcome::Skipped(_) => "SKIPPED",
        }
    }

    /// The reason for a result that is not a pass. Empty for a pass.
    pub fn reason(&self) -> &str {
        match self {
            Outcome::Pass => "",
            Outcome::Fail(reason) | Outcome::Pending(reason) | Outcome::Skipped(reason) => reason,
        }
    }

    fn status(&self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail(_) => "fail",
            Outcome::Pending(_) => "pending",
            Outcome::Skipped(_) => "skipped",
        }
    }

    fn from_status(status: &str, reason: &str) -> Option<Outcome> {
        let reason = reason.to_string();
        Some(match status {
            "pass" => Outcome::Pass,
            "fail" => Outcome::Fail(reason),
            "pending" => Outcome::Pending(reason),
            "skipped" => Outcome::Skipped(reason),
            _ => return None,
        })
    }
}

/// One check on one OS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub check: String,
    pub outcome: Outcome,
    /// Measurements and other facts from the run, for people reading the report.
    pub detail: String,
}

/// All checks of one run, on one OS and backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub platform: String,
    pub backend: String,
    pub results: Vec<CheckResult>,
}

impl RunReport {
    /// A run in which every check is pending, for an OS whose backend does not exist yet.
    pub fn all_pending(platform: &str, backend: &str, reason: &str) -> RunReport {
        RunReport {
            platform: platform.into(),
            backend: backend.into(),
            results: CHECKS
                .iter()
                .map(|check| CheckResult {
                    check: (*check).into(),
                    outcome: Outcome::Pending(reason.into()),
                    detail: String::new(),
                })
                .collect(),
        }
    }

    /// Counts of (pass, fail, pending, skipped).
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut counts = (0, 0, 0, 0);
        for result in &self.results {
            match result.outcome {
                Outcome::Pass => counts.0 += 1,
                Outcome::Fail(_) => counts.1 += 1,
                Outcome::Pending(_) => counts.2 += 1,
                Outcome::Skipped(_) => counts.3 += 1,
            }
        }
        counts
    }

    pub fn to_json(&self) -> Value {
        let results: Vec<Value> = self
            .results
            .iter()
            .map(|r| {
                json!({
                    "check": r.check,
                    "status": r.outcome.status(),
                    "reason": r.outcome.reason(),
                    "detail": r.detail,
                })
            })
            .collect();
        json!({ "platform": self.platform, "backend": self.backend, "results": results })
    }

    pub fn from_json(value: &Value) -> Result<RunReport, String> {
        let text = |v: &Value, field: &str| -> Result<String, String> {
            v.get(field)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("missing string field {field:?}"))
        };
        let platform = text(value, "platform")?;
        let backend = text(value, "backend")?;
        let items = value
            .get("results")
            .and_then(Value::as_array)
            .ok_or("missing results array")?;
        let mut results = Vec::new();
        for item in items {
            let status = text(item, "status")?;
            let reason = text(item, "reason")?;
            let outcome = Outcome::from_status(&status, &reason)
                .ok_or_else(|| format!("unknown status {status:?}"))?;
            results.push(CheckResult {
                check: text(item, "check")?,
                outcome,
                detail: text(item, "detail")?,
            });
        }
        Ok(RunReport {
            platform,
            backend,
            results,
        })
    }
}

/// Renders the compatibility report: a matrix of checks by OS, then totals and the reasons.
///
/// Columns are the runs in the order given. A check missing from a run shows as "not run".
pub fn render_markdown(runs: &[RunReport]) -> String {
    let mut names: Vec<String> = CHECKS.iter().map(|c| c.to_string()).collect();
    for run in runs {
        for result in &run.results {
            if !names.contains(&result.check) {
                names.push(result.check.clone());
            }
        }
    }

    let mut out = String::new();
    out.push_str("# Compatibility report\n\n");
    out.push_str(
        "Generated by `conformance-report` from the conformance suite (spec 14). Only PASS is\n\
         green. PENDING means the backend does not provide the feature yet. SKIPPED means the\n\
         check could not run in its session.\n\n",
    );

    out.push_str("| Check |");
    for run in runs {
        let _ = write!(out, " {} ({}) |", run.platform, run.backend);
    }
    out.push('\n');
    out.push_str("|---|");
    for _ in runs {
        out.push_str("---|");
    }
    out.push('\n');
    for name in &names {
        let _ = write!(out, "| {name} |");
        for run in runs {
            let cell = run
                .results
                .iter()
                .find(|r| &r.check == name)
                .map_or("not run", |r| r.outcome.label());
            let _ = write!(out, " {cell} |");
        }
        out.push('\n');
    }

    out.push_str("\n## Totals\n\n");
    for run in runs {
        let (pass, fail, pending, skipped) = run.counts();
        let _ = writeln!(
            out,
            "- {} ({}): {pass} pass, {fail} fail, {pending} pending, {skipped} skipped",
            run.platform, run.backend
        );
    }

    out.push_str("\n## Details\n\n");
    for name in &names {
        let mut lines = String::new();
        for run in runs {
            if let Some(result) = run.results.iter().find(|r| &r.check == name) {
                let reason = result.outcome.reason();
                let _ = write!(lines, "- {}: {}", run.platform, result.outcome.label());
                if !reason.is_empty() {
                    let _ = write!(lines, ". {reason}");
                }
                if !result.detail.is_empty() {
                    let _ = write!(lines, " ({})", result.detail);
                }
                lines.push('\n');
            }
        }
        if !lines.is_empty() {
            let _ = write!(out, "### {name}\n\n{lines}\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunReport {
        let mut run = RunReport::all_pending("windows", "webview2", "not run in this test");
        run.results[0].outcome = Outcome::Pass;
        run.results[0].detail = "200 round trips".into();
        run.results[1].outcome = Outcome::Fail("blob was short".into());
        run.results[2].outcome = Outcome::Skipped("no loader".into());
        run
    }

    #[test]
    fn a_run_survives_a_json_round_trip() {
        let run = sample();
        assert_eq!(RunReport::from_json(&run.to_json()), Ok(run));
    }

    #[test]
    fn an_unknown_status_is_refused() {
        let mut value = sample().to_json();
        value["results"][0]["status"] = json!("green");
        assert!(RunReport::from_json(&value).is_err());
    }

    #[test]
    fn counts_add_up_to_the_number_of_checks() {
        let run = sample();
        let (pass, fail, pending, skipped) = run.counts();
        assert_eq!((pass, fail, pending, skipped), (1, 1, 7, 1));
        assert_eq!(pass + fail + pending + skipped, CHECKS.len());
    }

    #[test]
    fn the_matrix_has_one_column_per_run_and_marks_missing_checks() {
        let linux = RunReport::all_pending("linux", "none", "no backend yet");
        let text = render_markdown(&[sample(), linux]);
        assert!(text.contains("| Check | windows (webview2) | linux (none) |"));
        assert!(text.contains("| ipc-round-trip | PASS | PENDING |"));
        assert!(text.contains("| large-blobs | FAIL | PENDING |"));
        assert!(text.contains("- windows: FAIL. blob was short"));
        assert!(text.contains("- windows (webview2): 1 pass, 1 fail, 7 pending, 1 skipped"));
    }
}
