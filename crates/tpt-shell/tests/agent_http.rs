// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! MCP over localhost HTTP in a running app (spec §10.4): a client POSTs a tool call to the app's
//! port, the app answers it on its UI thread, and the reply comes back over the socket. Skipped
//! unless `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(all(windows, feature = "mcp-http"))]

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_audit::Policy;
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

/// A token of the length the HTTP transport requires.
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

const MANIFEST: &str = "[app]\nname = \"agent-http-test\"\n\
    [agent]\nenabled = true\n\
    [agent.profiles.assistant]\nactions = [\"invoke\"]\ncommands = [\"ping\"]\n";

/// Posts one JSON-RPC message to `/mcp` and returns the whole response text.
fn post(port: u16, body: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
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

#[test]
fn a_tool_call_over_http_is_answered_by_the_running_app() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-agent-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).expect("the test folder");
    let manifest = base.join("tpt-shell.toml");
    std::fs::write(&manifest, MANIFEST).expect("the manifest");

    // A free port: bind, read the number, and let go. The app binds it again at start.
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    std::env::set_var("TPT_MCP_TOKEN", TOKEN);

    let app = App::new()
        .name("agent-http-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .window(Window::new("main").title("agent").size(320.0, 240.0))
        .audit(base.join("audit.jsonl"), Policy::default())
        .agent(&manifest, "assistant")
        .expect("the agent profile")
        .agent_tool(
            "ping",
            "answers with pong",
            tpt_shell_ipc::schema::Schema::Struct(&[]),
            |_| Ok(serde_json::json!("pong")),
        )
        .agent_http(port);
    let running = app.build().expect("the app should start");

    // The client runs on its own thread; the app answers from this thread's message loop.
    let client = std::thread::spawn(move || {
        post(
            port,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ping","arguments":{}}}"#,
        )
    });
    let finished = pump_until(Duration::from_secs(60), || client.is_finished());
    let reply = client.join().expect("the client thread");
    drop(running);
    let _ = std::fs::remove_dir_all(&base);
    std::env::remove_var("TPT_MCP_TOKEN");

    assert!(finished, "the call never finished");
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(
        reply.contains("pong"),
        "the tool's answer is in the reply: {reply}"
    );
    assert!(reply.contains("\"isError\":false"), "{reply}");
}
