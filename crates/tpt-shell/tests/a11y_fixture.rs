// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The accessibility audit gate (todo M8): the rules find every seeded violation on the live
//! fixture page, and report nothing on the elements the fixture marks clean. Skipped unless
//! `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(all(windows, feature = "test-driver"))]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};
use tpt_shell_test::channel::FileChannel;
use tpt_shell_test::driver::{role, Driver};
use tpt_shell_test::page_audit::{audit, trap, COLLECT_SCRIPT};

const INDEX: &str = include_str!("../../../fixtures/a11y-seeded/index.html");
const STYLE: &str = include_str!("../../../fixtures/a11y-seeded/style.css");
const APP_JS: &str = include_str!("../../../fixtures/a11y-seeded/app.js");
const EXPECTED: &str = include_str!("../../../fixtures/a11y-seeded/expected.json");

#[test]
fn the_audit_finds_every_seeded_violation_on_the_live_page() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-a11y-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let channel = FileChannel::create(&base).expect("the run directory");
    std::env::set_var("TPT_TEST_RUN", channel.dir());

    let app = App::new()
        .name("a11y-fixture-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", INDEX)
        .asset("style.css", "text/css", STYLE)
        .asset("app.js", "text/javascript", APP_JS)
        .window(Window::new("main").title("a11y").size(800.0, 900.0));
    let running = app.build().expect("the app should start");
    running.window("main").expect("the main window");

    let client = std::thread::spawn(move || {
        let mut driver = Driver::new(channel);
        // Wait until the page is up, then collect its data with the page's own script.
        driver
            .wait_for(&role("button").name("Save"), Duration::from_secs(20))
            .expect("the page is loaded");
        let raw = driver
            .eval(COLLECT_SCRIPT)
            .expect("the collector is answered");
        // The trap: open the dialog, then press Tab and record which element has focus, from
        // the element focused when the dialog opened.
        let open = driver
            .wait_for(
                &role("button").name("Open the dialog"),
                Duration::from_secs(20),
            )
            .expect("the dialog button is found");
        driver.click(&open).expect("the click is answered");
        let focused = "document.activeElement && document.activeElement.id";
        let mut trace = Vec::new();
        for _ in 0..4 {
            let id = driver.eval(focused).expect("focus is answered");
            trace.push(id.as_str().unwrap_or_default().to_string());
            driver.press("Tab").expect("the key press is answered");
        }
        let last = driver.eval(focused).expect("focus is answered");
        trace.push(last.as_str().unwrap_or_default().to_string());
        (raw, trace)
    });

    let finished = pump_until(Duration::from_secs(90), || client.is_finished());
    let (raw, trace) = client.join().expect("the driver thread");
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
    assert!(finished, "the audit never finished");

    let text = raw.as_str().expect("the collector returns JSON text");
    let page: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
    // The fixture is audited as if the user prefers reduced motion, which is the case its
    // reduced-motion seed describes.
    let mut found = audit(&page, true).expect("the audit runs");
    let exits = vec!["control-ok".to_string()];
    found.extend(trap("seed-trap-dialog", &trace, &exits));

    let expected: serde_json::Value = serde_json::from_str(EXPECTED).expect("expected.json");
    for v in expected["violations"].as_array().expect("violations") {
        let rule = v["rule"].as_str().unwrap();
        let id = v["id"].as_str().unwrap();
        assert!(
            found.iter().any(|f| f.rule == rule && f.id == id),
            "the audit missed {rule} on {id}; found {found:#?}; trace {trace:?}"
        );
    }
    for clean in expected["clean"].as_array().expect("clean") {
        let id = clean.as_str().unwrap();
        assert!(
            !found.iter().any(|f| f.id == id),
            "{id} is marked clean but was reported: {found:#?}"
        );
    }
}
