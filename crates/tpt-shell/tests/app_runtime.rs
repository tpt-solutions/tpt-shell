// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runs a real app on WebView2: its page calls a guarded command, and the reply comes back.
//! Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(windows)]

use std::time::Duration;

use tpt_shell::{App, CommandError, Grant, Scope, Target, Value, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

/// The start page. Its script is a separate asset, because the page policy blocks inline script.
const INDEX: &str = r#"<!doctype html><title>start</title><script src="app.js"></script>"#;

/// Asks for a file in scope, then one out of scope, and shows each reply in the title.
const APP_JS: &str = r#"
    var IN_SCOPE = '{"v":1,"id":1,"cmd":"read","args":{"path":"C:/data/notes.txt"}}';
    var OUT_OF_SCOPE = '{"v":1,"id":2,"cmd":"read","args":{"path":"C:/secrets/key"}}';
    var answers = 0;
    window.chrome.webview.addEventListener('message', function (e) {
      answers++;
      document.title = (answers === 1 ? '' : document.title + ' | ') + e.data;
      if (answers === 1) { window.chrome.webview.postMessage(OUT_OF_SCOPE); }
    });
    window.chrome.webview.postMessage(IN_SCOPE);
"#;

#[test]
fn an_app_window_calls_a_guarded_command_and_gets_the_reply() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-runtime-{}", std::process::id()));

    let app = App::new()
        .name("app-runtime-test")
        .loader(loader)
        .data_dir(&dir)
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", APP_JS)
        .window(Window::new("main").title("app-test").size(640.0, 480.0))
        .grant(Grant {
            capability: "fs.read".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Paths(vec!["C:/data".into()]),
            consent: false,
        })
        .command(
            "read",
            "fs.read",
            |args| match args["path"].as_str() {
                Some(path) => Target::Path(path.to_string()),
                None => Target::None,
            },
            |_| -> Result<Value, CommandError> { Ok(Value::String("contents".into())) },
        );

    let running = app.build().expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title.contains("CAP_OUT_OF_SCOPE")
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(got, "the second reply never arrived; title was {title:?}");
    let (first, second) = title.split_once(" | ").expect("two replies");
    assert!(
        first.contains(r#""data":"contents""#),
        "in-scope call: {first}"
    );
    assert!(
        second.contains(r#""code":"CAP_OUT_OF_SCOPE""#),
        "out of scope: {second}"
    );
}
