// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A page talks to a granted TCP device through the app's device hub (todo M7, the `device.net`
//! kind). The test starts a loopback echo server, grants the page that address, and checks the
//! echo comes back and the connection is audited. Needs the `net-devices` feature, since it opens
//! a socket. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is
//! available.

#![cfg(all(windows, feature = "net-devices"))]

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tpt_shell::device_scope::scope_from_grant;
use tpt_shell::devices::hub::{DeviceHub, EventKind};
use tpt_shell::devices::id::DeviceId;
use tpt_shell::{App, CommandError, Grant, Scope, Target, Value, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

#[test]
fn a_page_echoes_through_a_granted_tcp_device_and_the_connection_is_audited() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };

    // The echo server: one connection, which it answers with what it reads.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback listener");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the page connects");
        let mut buf = [0u8; 4];
        stream.read_exact(&mut buf).expect("the page writes");
        stream.write_all(&buf).expect("echo");
    });

    let dir = std::env::temp_dir().join(format!("tpt-app-net-{}", std::process::id()));
    let device_id: DeviceId = format!("net:127.0.0.1:{port}").parse().expect("id");
    let granted = Grant {
        capability: "device.net".into(),
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
        scope: Scope::Devices(vec![format!("net:127.0.0.1:{port}")]),
        consent: false,
    };
    let kinds: Arc<Mutex<Vec<EventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = kinds.clone();
    // No virtual devices: the network device is opened by the hub on demand.
    let hub = Rc::new(RefCell::new(DeviceHub::new(Vec::new(), move |event| {
        sink.lock().expect("kinds").push(event.kind)
    })));
    let for_command = hub.clone();

    let page = format!(
        r#"window.tpt.invoke('echo', {{ device: 'net:127.0.0.1:{port}', text: 'ping' }})
            .then(function (r) {{ document.title = 'done ' + r; }})
            .catch(function (e) {{ document.title = 'error ' + JSON.stringify(e); }});"#
    );
    let running = App::new()
        .name("app-net-test")
        .loader(loader)
        .data_dir(&dir)
        .grant(granted.clone())
        .command(
            "echo",
            "device.net",
            |args| match args["device"].as_str() {
                Some(device) => Target::Device(device.to_string()),
                None => Target::None,
            },
            move |args| -> Result<Value, CommandError> {
                let text = args["text"].as_str().unwrap_or("").to_string();
                let scope = scope_from_grant(&granted, &device_id).ok_or_else(|| {
                    CommandError::new("DEVICE_SCOPE", "the grant does not cover it")
                })?;
                let mut open = for_command
                    .borrow_mut()
                    .open_net(&device_id, &scope, Duration::from_secs(5))
                    .map_err(|e| CommandError::new("DEVICE_OPEN", &e.to_string()))?;
                open.write(text.as_bytes())
                    .map_err(|e| CommandError::new("DEVICE_WRITE", &e.to_string()))?;
                let back = open
                    .read(text.len())
                    .map_err(|e| CommandError::new("DEVICE_READ", &e.to_string()))?;
                Ok(Value::String(String::from_utf8_lossy(&back).into_owned()))
            },
        )
        .asset(
            "index.html",
            "text/html",
            r#"<!doctype html><title>start</title><script src="app.js"></script>"#,
        )
        .asset("app.js", "text/javascript", page)
        .window(Window::new("main").title("app-net-test"))
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
    server.join().expect("the echo server");

    assert!(answered, "the page got no answer; title was {title:?}");
    assert_eq!(title, "done ping");
    let audited = kinds.lock().expect("kinds").clone();
    assert!(audited.contains(&EventKind::Open), "{audited:?}");
    assert!(audited.contains(&EventKind::Write), "{audited:?}");
}
