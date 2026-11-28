// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An app whose audit log signs with a keystore key starts, and writes the key's public half
//! beside the log. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is
//! available.

#![cfg(windows)]

use tpt_shell::{App, Window};
use tpt_shell_audit::Policy;
use tpt_shell_platform_windows::window::runtime_test_loader;

#[test]
fn a_signed_audit_log_starts_and_writes_its_public_key() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    // The keystore lives under %LOCALAPPDATA%\tpt-shell\<app name>, so the name is unique to this
    // run and the folder is removed at the end.
    let app_name = format!("audit-key-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("tpt-audit-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test folder");
    let log = dir.join("audit.log");

    let running = App::new()
        .name(&app_name)
        .loader(loader)
        .data_dir(dir.join("data"))
        .audit(&log, Policy::default())
        .audit_signing_key("checkpoints")
        .window(Window::new("main").hidden())
        .build()
        .expect("the signed app should start");
    drop(running);

    let public = std::fs::read_to_string(dir.join("audit.log.pub")).expect("the public key file");
    assert!(
        tpt_shell_audit::verifying_key_from_hex(public.trim()).is_some(),
        "not a verifying key: {public}"
    );

    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let _ = std::fs::remove_dir_all(
            std::path::Path::new(&local)
                .join("tpt-shell")
                .join(&app_name),
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
