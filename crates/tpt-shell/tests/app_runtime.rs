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

#[test]
fn a_window_opens_where_it_was_saved_and_saves_where_it_is() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let state = dir.join("window-state.json");
    std::fs::write(
        &state,
        r#"{"main": {"x": 120, "y": 140, "width": 640, "height": 480, "maximized": false}}"#,
    )
    .expect("write state");

    let running = App::new()
        .name("app-state-test")
        .loader(loader)
        .data_dir(&dir)
        .remember_window_state(&state)
        .asset(
            "index.html",
            "text/html",
            "<!doctype html><title>state</title>",
        )
        .window(Window::new("main").title("app-state-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let placed = running.backend().placement(id).expect("placement");
    assert_eq!(
        (placed.x, placed.y, placed.width, placed.height),
        (120, 140, 640, 480),
        "the saved placement was not applied"
    );

    std::fs::remove_file(&state).expect("remove state");
    running.save_window_state().expect("save");
    let saved = std::fs::read_to_string(&state).expect("saved state");
    let _ = std::fs::remove_dir_all(&dir);
    drop(running);
    assert!(
        saved.contains("\"width\": 640") && saved.contains("\"maximized\": false"),
        "{saved}"
    );
}

#[test]
fn an_idle_kiosk_returns_to_its_home_page() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-idle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let secret = [7u8; 32];
    let public = tpt_shell_kiosk::config::public_key_hex(&secret);
    let body = br#"{"home":"index.html","idle_seconds":1}"#;
    let config = dir.join("kiosk.json");
    let sig = dir.join("kiosk.json.sig");
    std::fs::write(&config, body).expect("write config");
    std::fs::write(&sig, tpt_shell_kiosk::config::sign(body, &secret)).expect("write sig");

    // The window starts on another page, which sends nothing, so the kiosk goes home.
    let running = App::new()
        .name("app-idle-test")
        .loader(loader)
        .data_dir(&dir)
        .kiosk_config(&config, &sig, &public)
        .expect("kiosk config")
        .asset(
            "index.html",
            "text/html",
            "<!doctype html><title>home</title>",
        )
        .asset(
            "other.html",
            "text/html",
            "<!doctype html><title>other</title>",
        )
        .window(
            Window::new("main")
                .title("app-idle-test")
                .page("other.html"),
        )
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let home = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title == "home"
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(home, "the kiosk did not return home; title was {title:?}");
}

#[test]
fn app_state_is_saved_while_running_and_restored_at_the_next_start() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-snap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let state = dir.join("state.json");
    let _ = std::fs::remove_file(&state);

    // First run: the state is saved on a timer, and the file appears without any page.
    let first = App::new()
        .name("app-snap-test")
        .loader(loader.clone())
        .data_dir(&dir)
        .snapshot_state(
            &state,
            Duration::from_millis(100),
            || serde_json::json!({ "count": 42 }),
            |_| panic!("nothing is saved yet, so nothing is restored"),
        )
        .asset("index.html", "text/html", "<!doctype html><title>x</title>")
        .window(Window::new("main").title("app-snap-test"))
        .build()
        .expect("first start");
    let saved = pump_until(Duration::from_secs(30), || state.exists());
    drop(first);
    assert!(saved, "the state was never saved");

    // Second run: the saved value is handed to the restore callback before the windows show.
    let restored: std::rc::Rc<std::cell::RefCell<Option<serde_json::Value>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot = restored.clone();
    let second = App::new()
        .name("app-snap-test")
        .loader(loader)
        .data_dir(&dir)
        .snapshot_state(
            &state,
            Duration::from_secs(60),
            || serde_json::json!({ "count": 0 }),
            move |value| *slot.borrow_mut() = Some(value),
        )
        .asset("index.html", "text/html", "<!doctype html><title>x</title>")
        .window(Window::new("main").title("app-snap-test"))
        .build()
        .expect("second start");
    drop(second);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        restored.borrow().clone(),
        Some(serde_json::json!({ "count": 42 }))
    );
}

#[test]
fn a_page_prints_through_a_granted_device_and_the_use_is_audited() {
    use std::sync::{Arc, Mutex};
    use tpt_shell::device_scope::scope_from_grant;
    use tpt_shell::devices::escpos::{Align, Receipt};
    use tpt_shell::devices::hub::{DeviceHub, EventKind};
    use tpt_shell::devices::id::DeviceId;
    use tpt_shell::devices::virtual_devices::VirtualPrinter;

    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-device-{}", std::process::id()));
    let printer_id: DeviceId = "usb:0416:5011".parse().expect("id");
    // The grant the app is given. The command opens the printer with the scope derived from this
    // same grant, so the engine's check and the hub's check cannot disagree.
    let granted = Grant {
        capability: "device.print".into(),
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
        scope: Scope::Devices(vec!["usb:0416:5011".into()]),
        consent: false,
    };
    let (printer, output) = VirtualPrinter::new(printer_id.clone());
    let kinds: Arc<Mutex<Vec<EventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = kinds.clone();
    let hub = std::rc::Rc::new(std::cell::RefCell::new(DeviceHub::new(
        vec![Box::new(printer)],
        move |event| sink.lock().expect("kinds").push(event.kind),
    )));
    let for_command = hub.clone();

    let running = App::new()
        .name("app-device-test")
        .loader(loader)
        .data_dir(&dir)
        .grant(granted.clone())
        .command(
            "print",
            "device.print",
            |args| match args["device"].as_str() {
                Some(device) => Target::Device(device.to_string()),
                None => Target::None,
            },
            move |_args| -> Result<Value, CommandError> {
                let scope = scope_from_grant(&granted, &printer_id).ok_or_else(|| {
                    CommandError::new("DEVICE_SCOPE", "the grant does not cover it")
                })?;
                let mut receipt = Receipt::new();
                receipt
                    .align(Align::Center)
                    .bold(true)
                    .line("SHOP")
                    .expect("ascii")
                    .bold(false)
                    .align(Align::Left)
                    .line("Tea 2.50")
                    .expect("ascii")
                    .cut(false);
                let mut open = for_command
                    .borrow_mut()
                    .open(&printer_id, &scope)
                    .map_err(|e| CommandError::new("DEVICE_OPEN", &e.to_string()))?;
                open.write(&receipt.into_bytes())
                    .map_err(|e| CommandError::new("DEVICE_WRITE", &e.to_string()))?;
                Ok(Value::String("printed".into()))
            },
        )
        .asset(
            "index.html",
            "text/html",
            r#"<!doctype html><title>start</title><script src="app.js"></script>"#,
        )
        .asset(
            "app.js",
            "text/javascript",
            r#"window.tpt.invoke('print', { device: 'usb:0416:5011' })
                .then(function (r) { document.title = 'done ' + r; })
                .catch(function (e) { document.title = 'error ' + JSON.stringify(e); });"#,
        )
        .window(Window::new("main").title("app-device-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let answered = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title.starts_with("done") || title.starts_with("error")
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(answered, "the page got no answer; title was {title:?}");
    assert_eq!(title, "done printed");
    let preview = output.preview();
    assert!(
        preview.contains("*SHOP*") && preview.contains("Tea 2.50"),
        "{preview}"
    );
    let audited = kinds.lock().expect("kinds").clone();
    assert_eq!(
        audited,
        vec![EventKind::Open, EventKind::Write, EventKind::Close]
    );
}

#[test]
fn the_ui_handle_queues_work_for_known_windows_and_refuses_others() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-ui-{}", std::process::id()));
    let running = App::new()
        .name("ui-test")
        .loader(loader)
        .data_dir(&dir)
        .asset("index.html", "text/html", INDEX)
        .window(Window::new("main"))
        .build()
        .expect("the app should start");
    let ui = running.ui_handle();
    assert!(ui.show("missing").is_err(), "an unknown window is refused");
    ui.show("main").expect("a known window is queued");
    ui.emit("main", "ping", serde_json::json!({ "n": 1 }))
        .expect("an event is queued");
    // The handle is Send, so another thread can queue work too.
    let from_thread = ui.clone();
    std::thread::spawn(move || {
        from_thread
            .emit("main", "ping", serde_json::json!({}))
            .expect("queued from another thread")
    })
    .join()
    .expect("thread");
    // The queued work never runs here: the loop is not pumped, and dropping the app frees it.
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_kiosk_app_builds_with_its_page_failure_and_idle_hooks() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-kiosk-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    // Browser mode: a kiosk config without the single-app lock-down, so no fullscreen window and
    // no key hook. The idle reset and the page-failure hook are still installed.
    let secret = [7u8; 32];
    let public = tpt_shell_kiosk::config::public_key_hex(&secret);
    let body = br#"{"mode":"browser","home":"index.html","idle_seconds":1}"#;
    let config = dir.join("kiosk.json");
    let sig = dir.join("kiosk.json.sig");
    std::fs::write(&config, body).expect("write config");
    std::fs::write(&sig, tpt_shell_kiosk::config::sign(body, &secret)).expect("write sig");
    let running = App::new()
        .name("kiosk-run-test")
        .loader(loader)
        .data_dir(dir.join("data"))
        .asset("index.html", "text/html", INDEX)
        .window(Window::new("main"))
        .kiosk_config(&config, &sig, &public)
        .expect("the signed config is read")
        .build()
        .expect("the kiosk app should start");
    assert!(running.window("main").is_some());
    // The developer overlay is a separate window, never one of the app's windows.
    assert!(running.window("tpt-devtools").is_none());
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
