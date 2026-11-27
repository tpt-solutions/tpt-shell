// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A hidden window (todo M8, CI replay): the window is created but not shown, and its page still
//! loads and runs. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is
//! available.

#![cfg(windows)]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::backend::window_is_shown;
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

const INDEX: &str = r#"<!doctype html><title>starting</title>
<script src="app.js"></script>"#;

const APP_JS: &str = "document.title = 'loaded hidden';";

#[test]
fn a_hidden_window_is_not_shown_but_its_page_loads() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-hidden-window-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    let app = App::new()
        .name("hidden-window-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", APP_JS)
        .window(
            Window::new("main")
                .title("hidden")
                .size(320.0, 240.0)
                .hidden(),
        );
    let running = app.build().expect("the app should start");
    let id = running.window("main").expect("the main window exists");

    assert!(
        !window_is_shown(id),
        "a hidden window must not be on screen"
    );
    let loaded = pump_until(Duration::from_secs(30), || {
        running
            .backend()
            .document_title(id)
            .is_ok_and(|t| t == "loaded hidden")
    });
    let title = running
        .backend()
        .document_title(id)
        .unwrap_or_default()
        .to_string();
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        loaded,
        "the page of a hidden window did not run: title is {title:?}"
    );
}
