// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The test driver end to end (spec §10.6): a driver finds a button, clicks it, and takes a
//! screenshot of a running app, over the run directory. Skipped unless `TPT_WEBVIEW2_LOADER`
//! names WebView2Loader.dll and a desktop is available.

#![cfg(all(windows, feature = "test-driver"))]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};
use tpt_shell_test::channel::FileChannel;
use tpt_shell_test::driver::{role, Driver};

/// A page with one button. Its click changes the title, which the test reads back.
const INDEX: &str = r#"<!doctype html><title>start</title>
<button id="save">Save</button>
<script src="app.js"></script>"#;

const APP_JS: &str = r#"
    document.getElementById('save').addEventListener('click', function () {
      document.title = 'saved';
    });
"#;

/// The PNG file signature.
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[test]
fn a_driver_finds_clicks_and_captures_a_running_app() {
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

    // The driver runs on its own thread, as a test would; the app answers from this thread's
    // message loop, which is the UI thread here.
    let client = std::thread::spawn(move || {
        let mut driver = Driver::new(driver);
        let save = driver
            .wait_for(&role("button").name("Save"), Duration::from_secs(20))
            .expect("the button is found");
        driver.click(&save).expect("the click is answered");
        let png = driver
            .screenshot("after-click")
            .expect("the screenshot is answered");
        (save.handle(), png)
    });

    let mut title = String::new();
    let finished = pump_until(Duration::from_secs(60), || {
        title = running.backend().document_title(id).unwrap_or_default();
        client.is_finished() && title == "saved"
    });
    let (handle, png) = client.join().expect("the driver thread");
    drop(running);
    let _ = std::fs::remove_dir_all(&base);

    assert!(
        finished,
        "the click never reached the page; title was {title:?}"
    );
    assert!(handle > 0, "the button has a reference");
    assert!(png.starts_with(PNG_SIGNATURE), "the capture is a PNG");
    assert!(png.len() > 100, "the capture has content");
}
