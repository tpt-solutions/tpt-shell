// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A page talks to a granted USB bulk pipe through the app (todo M7, the `device.usb` kind): the
//! command opens the loopback pipe with the scope derived from the page's grant, writes, reads the
//! bytes back and returns them. Skipped unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and
//! a desktop is available.

#![cfg(windows)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tpt_shell::device_scope::scope_from_grant;
use tpt_shell::devices::hub::{DeviceHub, EventKind};
use tpt_shell::devices::id::DeviceId;
use tpt_shell::devices::virtual_devices::VirtualUsb;
use tpt_shell::{App, CommandError, Grant, Scope, Target, Value, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

#[test]
fn a_page_echoes_through_a_granted_usb_pipe_and_the_use_is_audited() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-usb-{}", std::process::id()));
    let port_id: DeviceId = "usb:0416:5011".parse().expect("id");
    let granted = Grant {
        capability: "device.usb".into(),
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
        scope: Scope::Devices(vec!["usb:0416:5011".into()]),
        consent: false,
    };
    let kinds: Arc<Mutex<Vec<EventKind>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = kinds.clone();
    let hub = Rc::new(RefCell::new(DeviceHub::new(
        vec![Box::new(VirtualUsb::loopback(port_id.clone()))],
        move |event| sink.lock().expect("kinds").push(event.kind),
    )));
    let for_command = hub.clone();

    let running = App::new()
        .name("app-usb-test")
        .loader(loader)
        .data_dir(&dir)
        .grant(granted.clone())
        .command(
            "echo",
            "device.usb",
            |args| match args["device"].as_str() {
                Some(device) => Target::Device(device.to_string()),
                None => Target::None,
            },
            move |args| -> Result<Value, CommandError> {
                let text = args["text"].as_str().unwrap_or("").to_string();
                let scope = scope_from_grant(&granted, &port_id).ok_or_else(|| {
                    CommandError::new("DEVICE_SCOPE", "the grant does not cover it")
                })?;
                let mut open = for_command
                    .borrow_mut()
                    .open(&port_id, &scope)
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
        .asset(
            "app.js",
            "text/javascript",
            r#"window.tpt.invoke('echo', { device: 'usb:0416:5011', text: 'ping' })
                .then(function (r) { document.title = 'done ' + r; })
                .catch(function (e) { document.title = 'error ' + JSON.stringify(e); });"#,
        )
        .window(Window::new("main").title("app-usb-test"))
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
    assert_eq!(title, "done ping");
    let audited = kinds.lock().expect("kinds").clone();
    assert!(audited.contains(&EventKind::Open), "{audited:?}");
    assert!(audited.contains(&EventKind::Write), "{audited:?}");
}
