// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Finds the licence page that the compliance step writes, so that `licenses.rs` can embed it.
//! The file is `target/tpt-shell/licenses.html` unless `TPT_SHELL_LICENSES` names another. When it
//! is missing, the build still succeeds and the app serves a short notice instead.

use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_licenses)");
    println!("cargo:rerun-if-env-changed=TPT_SHELL_LICENSES");
    let path = match std::env::var_os("TPT_SHELL_LICENSES") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("../../target/tpt-shell/licenses.html"),
    };
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_file() {
        println!("cargo:rustc-cfg=has_licenses");
        println!("cargo:rustc-env=TPT_SHELL_LICENSES_FILE={}", path.display());
    }
}
