// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The test driver end to end (spec §10.6): a driver finds elements, clicks, types, presses a key,
//! reads the DOM and runs a script, and takes a screenshot, against a running app. Skipped unless
//! `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(all(windows, feature = "test-driver"))]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};
use tpt_shell_test::channel::FileChannel;
use tpt_shell_test::driver::{role, Driver};

/// A page with a button and a text box. The page's key listener puts the last key in the title.
const INDEX: &str = r#"<!doctype html><title>start</title>
<button id="save">Save</button>
<input id="name" aria-label="Name">
<script src="app.js"></script>"#;

const APP_JS: &str = r#"
    document.addEventListener('keydown', function (e) {
      document.title = 'key:' + e.key;
    });
"#;

/// The PNG file signature.
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[test]
fn a_driver_finds_types_presses_clicks_and_captures_a_running_app() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-driver-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let driver = FileChannel::create(&base).expect("the run directory");
    // The app reads the run directory when it starts, so it is set before the app is built.
    std::env::set_var("TPT_TEST_RUN", driver.dir());

    let app = App::new()
        .name("test-driver-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", APP_JS)
        .window(Window::new("main").title("driver-test").size(640.0, 480.0));
    let running = app.build().expect("the app should start");
    let id = running.window("main").expect("the main window");

    // The driver runs on its own thread, as a test would. The app answers from this thread's
    // message loop, which is the UI thread here, so the loop is pumped until the driver is done.
    let client = std::thread::spawn(move || {
        let mut driver = Driver::new(driver);
        let save = driver
            .wait_for(&role("button").name("Save"), Duration::from_secs(20))
            .expect("the button is found");
        driver.click(&save).expect("the click is answered");
        let name = driver
            .wait_for(&role("textbox").name("Name"), Duration::from_secs(20))
            .expect("the text box is found");
        driver
            .type_text(&name, "Ada")
            .expect("the typing is answered");
        let typed = driver
            .eval("document.getElementById('name').value")
            .expect("the script is answered");
        driver.press("Enter").expect("the key press is answered");
        let html = driver.dom_snapshot().expect("the DOM is answered");
        let png = driver
            .screenshot("after-input")
            .expect("the screenshot is answered");
        (save.handle(), typed, html, png)
    });

    let finished = pump_until(Duration::from_secs(90), || client.is_finished());
    let title_after_press = running.backend().document_title(id).unwrap_or_default();
    let (handle, typed, html, png) = client.join().expect("the driver thread");
    drop(running);
    let _ = std::fs::remove_dir_all(&base);

    assert!(finished, "the driver never finished");
    assert!(handle > 0, "the button has a reference");
    assert_eq!(
        typed,
        serde_json::json!("Ada"),
        "the text box holds what was typed"
    );
    assert_eq!(
        title_after_press, "key:Enter",
        "the key press reached the page"
    );
    assert!(
        html["html"].as_str().is_some_and(|h| h.contains("Save")),
        "the DOM has the button: {html}"
    );
    assert!(png.starts_with(PNG_SIGNATURE), "the capture is a PNG");
    assert!(png.len() > 100, "the capture has content");
}
