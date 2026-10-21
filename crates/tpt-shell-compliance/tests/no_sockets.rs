// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The shell opens no sockets by default (spec §1, §10.8; todo "Runtime and air-gap").
//!
//! The test reads the normal and build dependency graph of the `tpt-shell` facade with its
//! default features, on every target, and checks that no crate known to open network sockets is
//! in it. A feature that adds one must be off by default; this test fails if one is turned on.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Crates that open network sockets or implement network protocols on top of them.
const NETWORK_CRATES: &[&str] = &[
    "async-io",
    "async-std",
    "attohttpc",
    "curl",
    "curl-sys",
    "h2",
    "hickory-resolver",
    "hyper",
    "isahc",
    "mio",
    "native-tls",
    "openssl",
    "openssl-sys",
    "quinn",
    "reqwest",
    "rustls",
    "socket2",
    "surf",
    "tokio",
    "tokio-tungstenite",
    "tungstenite",
    "trust-dns-resolver",
    "ureq",
];

fn default_graph_names() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "-p",
            "tpt-shell",
            "--target",
            "all",
            "-e",
            "normal,build",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .current_dir(&root)
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_default_graph_of_the_shell_links_no_network_crate() {
    let names = default_graph_names();
    assert!(
        names.contains("tpt-shell"),
        "the scan must see the facade itself"
    );
    let found: Vec<&str> = NETWORK_CRATES
        .iter()
        .copied()
        .filter(|n| names.contains(*n))
        .collect();
    assert!(
        found.is_empty(),
        "network crates in the default build: {found:?}. Put them behind a feature that is off by default."
    );
}
