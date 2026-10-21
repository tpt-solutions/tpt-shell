// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runs a real app on WebView2: its page calls a guarded command, and the reply comes back.
//! Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(windows)]

use std::time::Duration;

use tpt_shell::{embed_dir, App, CommandError, Grant, Scope, Target, Value, Window};
use tpt_shell_platform_windows::consent::answer_next_dialog;
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

#[test]
fn files_embedded_with_embed_dir_load_from_the_app_origin() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-embed-{}", std::process::id()));
    // fixtures/app: index.html loads app.js, which sets the title to "embedded-ok".
    let running = App::new()
        .name("embed-test")
        .loader(loader)
        .data_dir(&dir)
        .assets(embed_dir!("../../fixtures/app"))
        .window(Window::new("main").title("embed-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title == "embedded-ok"
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(got, "the embedded script never ran; title was {title:?}");
}

/// The Win32 answers to a Yes/No message box.
const IDYES: i32 = 6;
const IDNO: i32 = 7;

/// Asks for a file once. The grant needs consent, so the user is asked in a native dialog.
const CONSENT_JS: &str = r#"
    window.chrome.webview.addEventListener('message', function (e) { document.title = e.data; });
    window.chrome.webview.postMessage('{"v":1,"id":1,"cmd":"read","args":{"path":"C:/data/notes.txt"}}');
    "#;

/// Runs the consent app, answers its dialog with `answer`, and returns the page's title.
fn run_consent_app(name: &str, answer: i32) -> Option<String> {
    let (loader, _serial) = runtime_test_loader()?;
    let dir = std::env::temp_dir().join(format!("tpt-app-consent-{name}-{}", std::process::id()));
    let clicker = answer_next_dialog(answer);
    let running = App::new()
        .name(name)
        .loader(loader)
        .data_dir(&dir)
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", CONSENT_JS)
        .window(Window::new("main").title("consent-test"))
        .grant(Grant {
            capability: "fs.read".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Unrestricted,
            consent: true,
        })
        .command(
            "read",
            "fs.read",
            |_| Target::None,
            |_| -> Result<Value, CommandError> { Ok(Value::String("contents".into())) },
        )
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        !title.is_empty() && title != "start"
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(clicker.join().unwrap(), "the consent dialog never appeared");
    assert!(got, "the reply never arrived; title was {title:?}");
    Some(title)
}

#[test]
fn a_consent_grant_runs_the_command_when_the_user_says_yes() {
    let Some(title) = run_consent_app("consent-yes", IDYES) else {
        return;
    };
    assert!(title.contains(r#""ok":true"#), "after Yes: {title}");
    assert!(title.contains(r#""data":"contents""#), "after Yes: {title}");
}

#[test]
fn a_consent_grant_refuses_the_command_when_the_user_says_no() {
    let Some(title) = run_consent_app("consent-no", IDNO) else {
        return;
    };
    assert!(
        title.contains(r#""code":"CAP_CONSENT_DECLINED""#),
        "after No: {title}"
    );
}

/// The start page. Its script is a separate asset, because the page policy blocks inline script.
const LISTENER_HTML: &str =
    r#"<!doctype html><title>starting</title><script src="listen.js"></script>"#;

/// Registers a listener for `greet` and reports that it is ready.
const LISTENER_JS: &str = r#"
    window.tpt.on('greet', function (data) { document.title = 'got ' + data.name; });
    document.title = 'ready';
"#;

#[test]
fn an_event_from_rust_reaches_the_page_listener() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-event-{}", std::process::id()));
    let mut running = App::new()
        .name("app-event-test")
        .loader(loader)
        .data_dir(&dir)
        .asset("index.html", "text/html", LISTENER_HTML)
        .asset("listen.js", "text/javascript", LISTENER_JS)
        .window(Window::new("main").title("app-event-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let ready = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title == "ready"
    });
    assert!(
        ready,
        "the page never registered its listener; title was {title:?}"
    );

    running
        .emit("main", "greet", serde_json::json!({ "name": "rust" }))
        .expect("emit");
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title == "got rust"
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(got, "the event never reached the page; title was {title:?}");
}

/// Calls a command that does not exist, and shows the reply in the title.
const UNKNOWN_CALL_JS: &str = r#"
    window.chrome.webview.addEventListener('message', function (e) { document.title = e.data; });
    window.chrome.webview.postMessage('{"v":1,"id":1,"cmd":"no_such_command","args":{"x":1}}');
"#;

#[test]
fn a_command_call_from_a_page_is_written_to_the_audit_log() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-audit-{}", std::process::id()));
    let log = dir.join("audit.jsonl");
    let running = App::new()
        .name("app-audit-test")
        .loader(loader)
        .data_dir(&dir)
        .audit(&log, tpt_shell_audit::Policy::default())
        .asset(
            "index.html",
            "text/html",
            r#"<!doctype html><title>start</title><script src="app.js"></script>"#,
        )
        .asset("app.js", "text/javascript", UNKNOWN_CALL_JS)
        .window(Window::new("main").title("app-audit-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let answered = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title.contains("IPC_UNKNOWN_COMMAND")
    });
    drop(running);

    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let verified = tpt_shell_audit::verify_file(&log);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(answered, "the page got no reply; title was {title:?}");
    assert!(
        text.contains(r#""no_such_command""#),
        "entry missing: {text}"
    );
    assert!(
        text.contains("IPC_UNKNOWN_COMMAND"),
        "outcome missing: {text}"
    );
    assert!(verified.is_ok(), "the log does not verify: {verified:?}");
}

/// Asks for a blob through the page runtime, fetches it from its app URL, and shows the size and
/// media type in the title.
const BLOB_JS: &str = r#"
    window.tpt.invoke('make_blob').then(function (url) {
      return fetch(url).then(function (r) {
        return r.arrayBuffer().then(function (b) {
          document.title = 'len ' + b.byteLength + ' ' + r.headers.get('content-type');
        });
      });
    }).catch(function (e) { document.title = 'error ' + JSON.stringify(e); });
"#;

#[test]
fn a_blob_from_a_command_is_fetched_by_the_page_from_its_app_url() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-blob-{}", std::process::id()));
    let running = App::new()
        .name("app-blob-test")
        .loader(loader)
        .data_dir(&dir)
        .grant(Grant {
            capability: "blob.make".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Unrestricted,
            consent: false,
        })
        .command(
            "make_blob",
            "blob.make",
            |_| Target::None,
            |_| {
                tpt_shell::blob::put_blob("text/plain", vec![b'x'; 1000])
                    .map(|url| serde_json::json!(url))
                    .map_err(|e| CommandError::new("BLOB_FAILED", &e.to_string()))
            },
        )
        .asset(
            "index.html",
            "text/html",
            r#"<!doctype html><title>start</title><script src="app.js"></script>"#,
        )
        .asset("app.js", "text/javascript", BLOB_JS)
        .window(Window::new("main").title("app-blob-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title.starts_with("len ") || title.starts_with("error")
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(got, "the page never finished; title was {title:?}");
    assert_eq!(title, "len 1000 text/plain");
}
