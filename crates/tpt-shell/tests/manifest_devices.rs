// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An app started from its manifest creates the window the manifest declares, and opens the virtual
//! device the manifest names. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a
//! desktop is available.

#![cfg(windows)]

use tpt_shell::App;
use tpt_shell_devices::id::{DeviceId, DeviceScope};
use tpt_shell_platform_windows::window::runtime_test_loader;

#[test]
fn an_app_started_from_its_manifest_has_its_window_and_its_device() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-devices-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test folder");
    let manifest = dir.join("tpt-shell.toml");
    std::fs::write(
        &manifest,
        "[app]\nname = \"devices-test\"\n\
         [[windows]]\nname = \"main\"\ntitle = \"Devices\"\nwidth = 320\nheight = 240\n\
         [devices]\nvirtual = true\nvirtual_devices = [\"serial@serial:COM7\"]\n",
    )
    .expect("manifest");

    // The environment must not replace the manifest's list for this test.
    std::env::remove_var(tpt_shell_devices::virtual_devices::ENV_VAR);
    // The manifest declares the window and the device. The test adds neither in code.
    let mut running = App::from_manifest(&manifest)
        .expect("the manifest declares the app")
        .loader(loader)
        .data_dir(dir.join("data"))
        .build()
        .expect("the app should start");
    assert!(
        running.window("main").is_some(),
        "the manifest's window was not created"
    );
    let hub = running.devices().expect("the manifest names a device");
    let listed = hub.list(&DeviceScope::Serial {
        port: "COM*".into(),
    });
    let wanted: DeviceId = "serial:COM7".parse().expect("a device id");
    assert!(listed.contains(&wanted), "listed: {listed:?}");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
