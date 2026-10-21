// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! File-level conformance for profiles (spec §10.7): writes in profile A are invisible in
//! profile B, an ephemeral session leaves no files behind, and an export holds only its own
//! profile. WebView2 storage is checked by the Windows backend tests, not here.

use std::fs;
use std::path::PathBuf;

use tpt_shell_profiles::export::read_export;
use tpt_shell_profiles::{Ephemeral, ProfileStore};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tpt-profiles-conformance-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn writes_in_one_named_profile_are_invisible_in_another() {
    let base = scratch("isolation");
    let store = ProfileStore::new(&base);
    let a = store.create("a").expect("a");
    let b = store.create("b").expect("b");
    fs::write(a.join("Cookies"), "session=1").expect("write a");
    fs::write(a.join("consent.txt"), "camera=allow").expect("write a");

    assert!(!b.join("Cookies").exists());
    assert!(!b.join("consent.txt").exists());
    assert_eq!(store.usage("b").expect("usage b"), 0);
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn an_ephemeral_session_leaves_no_files_behind() {
    let base = scratch("ephemeral");
    let session_path = {
        let session = Ephemeral::create(&base).expect("session");
        fs::write(session.path().join("Local Storage"), "state").expect("write");
        session.path().to_path_buf()
    };
    assert!(!session_path.exists());
    let leftover = fs::read_dir(base.join("ephemeral"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(leftover, 0, "no session folder remains");
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn an_export_holds_only_its_own_profile() {
    let base = scratch("export");
    let store = ProfileStore::new(&base);
    let a = store.create("a").expect("a");
    store.create("b").expect("b");
    fs::write(a.join("data.txt"), "only a").expect("write");
    let out = base.join("a.tptprof");
    store.export("a", &out, "long enough pass").expect("export");

    let files = read_export(&out, "long enough pass").expect("open export");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "data.txt");
    let _ = fs::remove_dir_all(&base);
}
