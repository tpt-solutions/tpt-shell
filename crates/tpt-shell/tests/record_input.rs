// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A recording captures the page's input (spec §10.5). The page dispatches a click, an input change
//! and a key press on load; the page listener reports each one, and the recording written when the
//! app quits holds them. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a
//! desktop is available.

#![cfg(windows)]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::runtime_test_loader;
use tpt_shell_replay::Mode;

const INDEX: &str = r#"<!doctype html><title>record</title>
<button id="save">Save</button>
<input id="name" aria-label="Name">
<script src="app.js"></script>"#;

/// Acts once the page has loaded, so the listener the app installed sees each event.
const APP_JS: &str = r#"
    window.addEventListener('load', function () {
      document.getElementById('save').click();
      var name = document.getElementById('name');
      name.value = 'Ada';
      name.dispatchEvent(new Event('input', { bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
"#;

#[test]
fn a_recording_holds_the_page_input_the_listener_reported() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-record-input-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("the test folder");
    let file = base.join("run.tptreplay");

    let app = App::new()
        .name("record-input-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", APP_JS)
        .window(Window::new("main").title("record").size(320.0, 240.0))
        .record(&file);
    let running = app.build().expect("the app should start");
    let ui = running.ui_handle();

    // The page acts as soon as it loads. Quitting after a few seconds ends the run, and the run
    // writes the recording.
    let quitter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        ui.quit().expect("the quit is posted");
    });
    running.run().expect("the run ends");
    quitter.join().expect("the quitter");

    let bytes = std::fs::read(&file).expect("the recording was written");
    let replay = tpt_shell_replay::read_compressed(&bytes, Mode::Lenient).expect("it reads back");
    let _ = std::fs::remove_dir_all(&base);
    let inputs: Vec<&serde_json::Value> = replay
        .events
        .iter()
        .filter(|e| e.kind == "input")
        .map(|e| &e.data)
        .collect();
    let kind_of = |k: &str| inputs.iter().any(|d| d["kind"] == serde_json::json!(k));
    assert!(kind_of("click"), "a click was recorded: {inputs:?}");
    assert!(kind_of("input"), "an input change was recorded: {inputs:?}");
    assert!(kind_of("key"), "a key press was recorded: {inputs:?}");
    assert!(
        inputs
            .iter()
            .any(|d| d["kind"] == "click" && d["path"][0]["name"] == "Save"),
        "the click names the Save button: {inputs:?}"
    );
    assert!(
        inputs
            .iter()
            .any(|d| d["kind"] == "input" && d["value_length"] == 3),
        "the input change records the length, not the text: {inputs:?}"
    );
    assert!(
        inputs
            .iter()
            .any(|d| d["kind"] == "key" && d["key"] == "Enter"),
        "the key press names the key: {inputs:?}"
    );
}
