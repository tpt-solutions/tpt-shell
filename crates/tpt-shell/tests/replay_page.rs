// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Replays a recording's page input into a fresh window (spec §10.5). A first run records a page
//! that clicks, types and presses Enter on load. A second app, whose page does nothing on its own,
//! replays that recording: its click and key are performed on the page, and the report says what
//! happened. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is
//! available.

#![cfg(all(windows, feature = "agent"))]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::runtime_test_loader;

/// The page that acts on load, for the recording.
const ACTING_JS: &str = r#"
    window.addEventListener('load', function () {
      document.getElementById('save').click();
      var name = document.getElementById('name');
      name.value = 'Ada';
      name.dispatchEvent(new Event('input', { bubbles: true }));
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
"#;

/// The page the replay runs on. Its click and key set the title, which the report shows.
const QUIET_JS: &str = r#"
    document.getElementById('save').addEventListener('click', function () {
      document.title = 'clicked';
    });
    document.addEventListener('keydown', function (e) {
      document.title = 'key:' + e.key;
    });
"#;

const INDEX: &str = r#"<!doctype html><title>page</title>
<button id="save">Save</button>
<input id="name" aria-label="Name">
<script src="app.js"></script>"#;

#[test]
fn a_recorded_click_and_key_are_performed_on_a_fresh_page() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-replay-page-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("the test folder");
    let file = base.join("run.tptreplay");

    // Record: the acting page dispatches its own events, and the app quits after a few seconds.
    let recorded = App::new()
        .name("replay-page-test")
        .loader(loader.clone())
        .data_dir(base.join("record-data"))
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", ACTING_JS)
        .window(Window::new("main").title("page").size(320.0, 240.0))
        .record(&file);
    let running = recorded.build().expect("the recording run starts");
    let ui = running.ui_handle();
    let quitter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        ui.quit().expect("the quit is posted");
    });
    running.run().expect("the recording run ends");
    quitter.join().expect("the quitter");
    assert!(file.exists(), "the recording was written");

    // Replay: a page that does nothing on its own performs the recorded click and key.
    let report = App::new()
        .name("replay-page-test")
        .loader(loader)
        .data_dir(base.join("replay-data"))
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", QUIET_JS)
        .window(Window::new("main").title("page").size(320.0, 240.0))
        .replay_page_input(&file)
        .expect("the replay runs");
    let _ = std::fs::remove_dir_all(&base);

    let performed = |want: &str| report.iter().any(|l| l.contains(want) && l.ends_with("ok"));
    assert!(
        performed("click button \"Save\""),
        "the click was performed: {report:?}"
    );
    assert!(
        report.iter().any(|l| l.starts_with("key Enter")),
        "the key was performed: {report:?}"
    );
    assert!(
        report
            .iter()
            .any(|l| l.contains("not replayed") && l.contains("length of the text")),
        "the typed text is reported as not replayed: {report:?}"
    );
    assert_eq!(
        report.last().map(String::as_str),
        Some("title after the replay: key:Enter"),
        "the page has the key press last: {report:?}"
    );
}
