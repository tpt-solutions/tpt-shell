// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An agent's click on a link is checked against the profile's routes before it runs (spec
//! §10.4). A link outside the routes is refused and the page stays put; a link inside them is
//! followed. Runs in a real app on WebView2 over the MCP HTTP transport. Skipped unless
//! `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(all(windows, feature = "mcp-http"))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::Duration;

use serde_json::Value;
use tpt_shell::{App, Window};
use tpt_shell_audit::Policy;
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// The profile may click and wait for elements, and may only follow links into the app.
const MANIFEST: &str = "[app]\nname = \"agent-links-test\"\n\
    [agent]\nenabled = true\n\
    [agent.profiles.assistant]\nactions = [\"click\", \"wait_for\"]\n\
    routes = [\"https://tpt-app.localhost/*\"]\n";

const INDEX: &str = r#"<!doctype html><title>start</title>
<a href="https://example.com/">Outside</a> <a href="help.html">Help</a>"#;
const HELP: &str = r#"<!doctype html><title>help</title><p>Help page</p>"#;

/// Posts one JSON-RPC message to `/mcp` and returns the whole response text.
fn post(port: u16, body: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("timeout");
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {TOKEN}\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("send");
    let mut out = String::new();
    stream.read_to_string(&mut out).expect("read");
    out
}

/// Makes one tool call. The client runs on its own thread while the app answers on this one.
fn call(running: &mut tpt_shell::Running, port: u16, body: String) -> Value {
    let client = std::thread::spawn(move || post(port, &body));
    let finished = pump_until(Duration::from_secs(60), || client.is_finished());
    let reply = client.join().expect("the client thread");
    assert!(finished, "the call never finished");
    let _ = running;
    let json = reply.split("\r\n\r\n").nth(1).expect("a body");
    serde_json::from_str(json).expect("a JSON-RPC reply")
}

/// The tool's result text, parsed as JSON. Page text comes wrapped in `<untrusted>` markers, which
/// are taken off first.
fn result_of(reply: &Value) -> Value {
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("null");
    let text = text
        .strip_prefix("<untrusted>")
        .and_then(|t| t.strip_suffix("</untrusted>"))
        .unwrap_or(text);
    serde_json::from_str(text).unwrap_or(Value::String(text.to_string()))
}

/// The element reference of the element with this role and name, from the snapshot the agent sees.
fn element_ref(running: &mut tpt_shell::Running, port: u16, role: &str, name: &str) -> u64 {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "wait_for",
                    "arguments": { "role": role, "name": name, "timeout_ms": 15000, "poll_ms": 50 } }
    })
    .to_string();
    let reply = call(running, port, body);
    let result = result_of(&reply);
    result["ref"]
        .as_u64()
        .unwrap_or_else(|| panic!("no ref for {name:?} in {reply}"))
}

#[test]
fn a_click_on_a_link_is_checked_against_the_routes() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-agent-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("the test folder");
    let manifest = base.join("tpt-shell.toml");
    std::fs::write(&manifest, MANIFEST).expect("the manifest");
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    std::env::set_var("TPT_MCP_TOKEN", TOKEN);

    let mut running = App::new()
        .name("agent-links-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", INDEX)
        .asset("help.html", "text/html", HELP)
        .window(Window::new("main").title("start"))
        .audit(base.join("audit.jsonl"), Policy::default())
        .agent(&manifest, "assistant")
        .expect("the agent profile")
        .agent_http(port)
        .build()
        .expect("the app should start");

    // The snapshot needs the page loaded: wait for its title before the agent looks.
    let id = running.window("main").expect("the main window");
    let loaded = pump_until(Duration::from_secs(30), || {
        running.backend().document_title(id).unwrap_or_default() == "start"
    });
    assert!(loaded, "the start page never loaded");

    // The outside link is refused, and the page does not move.
    let outside = element_ref(&mut running, port, "link", "Outside");
    let refused = call(
        &mut running,
        port,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "click", "arguments": { "ref": outside } }
        })
        .to_string(),
    );
    assert_eq!(refused["result"]["isError"], Value::Bool(true), "{refused}");
    let text = refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(text.contains("routes do not allow"), "{text}");
    let title = running.backend().document_title(id).unwrap_or_default();
    assert_eq!(title, "start", "a refused click must not navigate");

    // The link inside the routes is followed.
    let help = element_ref(&mut running, port, "link", "Help");
    let followed = call(
        &mut running,
        port,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "click", "arguments": { "ref": help } }
        })
        .to_string(),
    );
    assert_eq!(
        followed["result"]["isError"],
        Value::Bool(false),
        "{followed}"
    );
    let arrived = pump_until(Duration::from_secs(30), || {
        running.backend().document_title(id).unwrap_or_default() == "help"
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
    std::env::remove_var("TPT_MCP_TOKEN");
    assert!(arrived, "the followed link never loaded");
}

/// A button whose script navigates the page. Clicking it is not a link or a form, so the click
/// check passes it; the window's navigation list must cancel the navigation. The script is a file,
/// because the page policy blocks inline script.
const SCRIPT_NAV: &str = r#"<!doctype html><title>start</title><button id="go">Go</button>
<script src="nav.js"></script>"#;
const NAV_JS: &str = r#"document.getElementById('go').addEventListener('click', function () {
    document.title = 'clicked';
    location.href = 'https://example.com/';
});"#;

#[test]
fn a_script_navigation_from_a_click_is_cancelled_on_the_agent_window() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-agent-script-nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("the test folder");
    let manifest = base.join("tpt-shell.toml");
    std::fs::write(&manifest, MANIFEST).expect("the manifest");
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    std::env::set_var("TPT_MCP_TOKEN", TOKEN);

    // The window itself allows example.com. The agent profile's routes allow only the app, so the
    // agent's window must cancel the navigation anyway.
    let mut running = App::new()
        .name("agent-script-nav-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .asset("index.html", "text/html", SCRIPT_NAV)
        .asset("nav.js", "text/javascript", NAV_JS)
        .window(
            Window::new("main")
                .title("start")
                .allow_navigation(&["https://example.com/"]),
        )
        .audit(base.join("audit.jsonl"), Policy::default())
        .agent(&manifest, "assistant")
        .expect("the agent profile")
        .agent_http(port)
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let loaded = pump_until(Duration::from_secs(30), || {
        running.backend().document_title(id).unwrap_or_default() == "start"
    });
    assert!(loaded, "the start page never loaded");

    let go = element_ref(&mut running, port, "button", "Go");
    let clicked = call(
        &mut running,
        port,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "click", "arguments": { "ref": go } }
        })
        .to_string(),
    );
    assert_eq!(
        clicked["result"]["isError"],
        Value::Bool(false),
        "{clicked}"
    );

    // Give the navigation time to start and be cancelled, then check the page did not move.
    pump_until(Duration::from_secs(4), || false);
    let shown = tpt_shell_platform_windows::backend::page_source(id).unwrap_or_default();
    let title = running.backend().document_title(id).unwrap_or_default();
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
    std::env::remove_var("TPT_MCP_TOKEN");
    // The click reached the page, so the navigation was attempted; then the page stayed put.
    assert_eq!(title, "clicked", "the click's script did not run");
    assert!(
        shown.starts_with("https://tpt-app.localhost"),
        "the script's navigation was not cancelled: now showing {shown:?}"
    );
}
