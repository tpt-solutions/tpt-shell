// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The conformance suite as a test target. Every OS runs it: `cargo test --manifest-path
//! conformance/Cargo.toml`.
//!
//! Why a test target and not a binary: the checks need a real window and the OS test harness
//! already runs on each CI job, so each OS gets the same entry point and the same exit status.
//!
//! Output: one JSON file per OS in `TPT_CONFORMANCE_OUT` (default `conformance/out`), named
//! `<os>.json`. `TPT_CONFORMANCE_LABEL` replaces the OS name for a run on another setup (the
//! headless Wayland job). `conformance-report` merges the files into the compatibility report.
//!
//! Exit status: the test fails when a check that ran did not pass. Pending and skipped checks are
//! written to the report but do not fail the run, so a backend can land one check at a time.
//! Set `TPT_CONFORMANCE_REQUIRE_PASS=1` to fail on pending and skipped checks as well. Use it on a
//! platform once its gate closes.

use std::path::PathBuf;

use tpt_shell_conformance::{Outcome, RunReport};

#[cfg(windows)]
#[path = "conformance/windows.rs"]
mod windows;

/// The run for this OS. macOS and Linux have no backend yet, so every check is pending there.
fn run_suite() -> RunReport {
    #[cfg(windows)]
    {
        windows::run()
    }
    #[cfg(not(windows))]
    {
        RunReport::all_pending(
            std::env::consts::OS,
            "none",
            "no backend for this OS yet (M4 for macOS, M5 for Linux)",
        )
    }
}

#[test]
fn conformance_suite() {
    let mut run = run_suite();
    // A job that runs the suite on a different setup (such as headless Wayland) names its result.
    if let Ok(label) = std::env::var("TPT_CONFORMANCE_LABEL") {
        run.platform = label;
    }

    let out_dir = std::env::var_os("TPT_CONFORMANCE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out"));
    std::fs::create_dir_all(&out_dir).expect("create the result directory");
    let file = out_dir.join(format!("{}.json", run.platform));
    let text = serde_json::to_string_pretty(&run.to_json()).expect("serialise the run");
    std::fs::write(&file, text).expect("write the result file");

    for result in &run.results {
        eprintln!(
            "{:<22} {:<8} {} {}",
            result.check,
            result.outcome.label(),
            result.outcome.reason(),
            result.detail
        );
    }
    eprintln!("result file: {}", file.display());

    let failed: Vec<&str> = run
        .results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail(_)))
        .map(|r| r.check.as_str())
        .collect();
    assert!(failed.is_empty(), "conformance checks failed: {failed:?}");

    if std::env::var("TPT_CONFORMANCE_REQUIRE_PASS").as_deref() == Ok("1") {
        let not_passing: Vec<&str> = run
            .results
            .iter()
            .filter(|r| r.outcome != Outcome::Pass)
            .map(|r| r.check.as_str())
            .collect();
        assert!(
            not_passing.is_empty(),
            "checks not passing with TPT_CONFORMANCE_REQUIRE_PASS=1: {not_passing:?}"
        );
    }
}
