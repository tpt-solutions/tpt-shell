// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `include_manifest!` expands at compile time to the manifest's text. These tests expand it on a
//! real template manifest and check that the text is the file and parses to the same app.

use tpt_shell::include_manifest;

/// The vanilla template's manifest, embedded at compile time. The macro checks it too, so a
/// broken template would fail this test's build.
const VANILLA: &str = include_manifest!("../../templates/vanilla/tpt-shell.toml");

#[test]
fn the_expansion_is_the_manifest_file() {
    let on_disk = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../templates/vanilla/tpt-shell.toml"
    ))
    .expect("the template manifest");
    assert_eq!(VANILLA, on_disk);
}

#[test]
fn the_expansion_parses_to_the_template_app() {
    let manifest = tpt_shell_manifest::parse(VANILLA).expect("the template manifest parses");
    assert_eq!(manifest.app.name, "my-app");
    assert!(manifest.windows.iter().any(|w| w.name == "main"));
}
