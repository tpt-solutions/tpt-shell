// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The WebView2 release channel an app asks for (todo M8 and M1). Stable is installed on the
//! development desktop, so an app asking for it builds. A channel that is not installed is refused
//! at build with its name in the message. Skipped unless `TPT_WEBVIEW2_LOADER` names
//! WebView2Loader.dll and a desktop is available.

#![cfg(windows)]

use tpt_shell::{App, Error, WebView2Channel, Window};
use tpt_shell_platform_windows::channel::installed_version;
use tpt_shell_platform_windows::loader::OfficialLoader;
use tpt_shell_platform_windows::window::runtime_test_loader;

#[test]
fn an_app_on_the_stable_channel_builds() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-channel-stable-{}", std::process::id()));
    let running = App::new()
        .name("channel-stable")
        .loader(loader)
        .data_dir(base.join("data"))
        .webview2_channel(WebView2Channel::Stable)
        .window(Window::new("main"))
        .build()
        .expect("the stable runtime is installed, so the app builds");
    assert!(running.window("main").is_some());
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn an_app_asking_for_a_missing_channel_is_refused_by_name() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let official = OfficialLoader::load(std::path::Path::new(&loader)).expect("the loader");
    if installed_version(&official, WebView2Channel::Canary).is_ok() {
        eprintln!("skipped: the canary channel is installed on this machine");
        return;
    }
    let base = std::env::temp_dir().join(format!("tpt-channel-canary-{}", std::process::id()));
    let result = App::new()
        .name("channel-canary")
        .loader(loader)
        .data_dir(base.join("data"))
        .webview2_channel(WebView2Channel::Canary)
        .window(Window::new("main"))
        .build();
    let _ = std::fs::remove_dir_all(&base);
    match result {
        Err(Error::Backend(message)) => {
            assert!(message.contains("canary"), "{message}");
            assert!(message.contains("not installed"), "{message}");
        }
        Err(other) => panic!("unexpected error {other:?}"),
        Ok(_) => panic!("a missing channel must not build"),
    }
}
