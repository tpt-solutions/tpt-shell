// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! End-to-end tests of the licence gate on fixtures: a fixture lock file, the metadata for it,
//! and a `deny.toml`. A deliberately bad dependency must fail the gate, and the failure must
//! carry its code and name the crate. The last test runs the gate on this workspace.

use std::path::Path;
use std::process::Command;

use tpt_shell_compliance::lock::sourced_packages;
use tpt_shell_compliance::{
    allow_list_from_deny_toml, app_from_metadata, attestation_with_allow, components_from_metadata,
    gate, gate_failure, Reproducibility, CODE_GATE,
};

const DENY: &str = include_str!("fixtures/deny.toml");
const LOCK: &str = include_str!("fixtures/fixture.lock");
const GOOD: &str = include_str!("fixtures/metadata-good.json");
const BAD: &str = include_str!("fixtures/metadata-bad.json");

fn repro() -> Reproducibility {
    Reproducibility {
        source_date_epoch: None,
        locked: true,
        cargo_flags: vec!["--locked".into()],
    }
}

#[test]
fn a_clean_fixture_build_passes_the_gate() {
    let allow = allow_list_from_deny_toml(DENY).unwrap();
    let mut locked = sourced_packages(LOCK).unwrap();
    locked.retain(|(name, _)| name != "fixture-copyleft");
    let components = components_from_metadata(GOOD).unwrap();
    let violations = gate(&components, &locked, &allow);
    assert!(violations.is_empty(), "{violations:?}");
    let att = attestation_with_allow(
        &app_from_metadata(GOOD, "tpt-fixture-app").unwrap(),
        "rustc test",
        "0",
        &[],
        &violations,
        None,
        &repro(),
        &allow,
    );
    assert_eq!(att["policy"]["result"], "pass");
}

#[test]
fn a_bad_dependency_fails_the_gate_end_to_end() {
    let allow = allow_list_from_deny_toml(DENY).unwrap();
    let locked = sourced_packages(LOCK).unwrap();
    let components = components_from_metadata(BAD).unwrap();
    let violations = gate(&components, &locked, &allow);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("fixture-copyleft"));
    assert!(violations[0].contains("GPL-3.0-only"));

    let failure = gate_failure(&violations);
    assert!(failure.starts_with(CODE_GATE));
    let att = attestation_with_allow(
        &app_from_metadata(BAD, "tpt-fixture-app").unwrap(),
        "rustc test",
        "0",
        &[],
        &violations,
        None,
        &repro(),
        &allow,
    );
    assert_eq!(att["policy"]["result"], "fail");
}

#[test]
fn a_locked_crate_without_metadata_fails_the_gate() {
    let allow = allow_list_from_deny_toml(DENY).unwrap();
    let locked = sourced_packages(LOCK).unwrap();
    let components = components_from_metadata(GOOD).unwrap();
    let violations = gate(&components, &locked, &allow);
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(violations[0].contains("fixture-copyleft"));
}

#[test]
fn the_workspace_passes_the_gate_with_its_own_deny_toml() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(&root)
        .output()
        .expect("cargo runs");
    assert!(output.status.success(), "cargo metadata failed");
    let metadata = String::from_utf8_lossy(&output.stdout).to_string();
    let deny = std::fs::read_to_string(root.join("deny.toml")).unwrap();
    let allow = allow_list_from_deny_toml(&deny).unwrap();
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let violations = gate(
        &components_from_metadata(&metadata).unwrap(),
        &sourced_packages(&lock).unwrap(),
        &allow,
    );
    assert!(violations.is_empty(), "{violations:?}");
}
