// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! SBOM schema validation (spec §10.8). The SBOMs this workspace produces are checked against
//! the published CycloneDX 1.5 and SPDX 2.3 schemas.
//!
//! The schemas are not in the repository. Point `TPT_SBOM_SCHEMAS` at a directory that holds
//! `bom-1.5.schema.json` and `spdx-2.3.schema.json` (the names `tools/validate-sbom.sh` uses).
//! Without them the test prints why it skipped and passes. The check follows the keywords in
//! `tpt_shell_compliance::schema`; the full ajv check is `tools/validate-sbom.sh`.

use std::path::{Path, PathBuf};
use std::process::Command;

use tpt_shell_compliance::schema::check;
use tpt_shell_compliance::{
    app_from_metadata, components_from_metadata, cyclonedx, iso8601_utc, spdx,
};

fn read_json(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("schema file reads");
    serde_json::from_str(&text).expect("schema file is JSON")
}

#[test]
fn generated_sboms_match_the_published_schemas() {
    let Some(dir) = std::env::var_os("TPT_SBOM_SCHEMAS").map(PathBuf::from) else {
        eprintln!("skipped: set TPT_SBOM_SCHEMAS to the directory of the published SBOM schemas");
        return;
    };
    let cdx_schema = dir.join("bom-1.5.schema.json");
    let spdx_schema = dir.join("spdx-2.3.schema.json");
    if !cdx_schema.exists() || !spdx_schema.exists() {
        eprintln!(
            "skipped: {} or {} is missing",
            cdx_schema.display(),
            spdx_schema.display()
        );
        return;
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(&root)
        .output()
        .expect("cargo runs");
    assert!(output.status.success(), "cargo metadata failed");
    let metadata = String::from_utf8_lossy(&output.stdout).to_string();
    let app = app_from_metadata(&metadata, "tpt-shell").unwrap();
    let components = components_from_metadata(&metadata).unwrap();
    let created = iso8601_utc(0);

    let bom = cyclonedx(&components, &app, Some(&created));
    let errors = check(&bom, &read_json(&cdx_schema));
    assert!(errors.is_empty(), "CycloneDX: {errors:#?}");

    let doc = spdx(
        &components,
        &app,
        "https://example.invalid/spdx/schema-test",
        Some(&created),
    );
    let errors = check(&doc, &read_json(&spdx_schema));
    assert!(errors.is_empty(), "SPDX: {errors:#?}");
}
