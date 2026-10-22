// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! MCP over localhost HTTP (spec §10.4). Compiled only with the `mcp-http` feature, which is off
//! by default, because it opens a socket.
//!
//! Rules: the listener binds to 127.0.0.1 only; every request needs `Authorization: Bearer
//! <token>` with a token of at least 32 characters; the `Host` header must name the loopback
//! address (this blocks DNS rebinding); requests are one JSON-RPC message in a POST to `/mcp`.
//! The token is compared in constant time. Anything else gets an HTTP error and no session call.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::{Confirm, Session};

/// The token is too short, or the port cannot be bound on loopback. Fix: give a token of at
/// least 32 characters, and a free port.
///
/// Support: Full.
pub const CODE_HTTP: &str = "TS0111";
/// Minimum token length, in characters.
///
/// Support: Full.
pub const TOKEN_MIN_LEN: usize = 32;

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 64 * 1024;

/// Binds `127.0.0.1:port`. Port 0 picks a free port; read it back with `local_addr`.
///
/// Support: Full.
pub fn bind_loopback(port: u16, token: &str) -> Result<TcpListener, String> {
    check_token(token)?;
    TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|e| format!("{CODE_HTTP}: cannot bind 127.0.0.1:{port}: {e}"))
}

fn check_token(token: &str) -> Result<(), String> {
    if token.chars().count() < TOKEN_MIN_LEN {
        return Err(format!(
            "{CODE_HTTP}: the MCP token has {} characters; at least {TOKEN_MIN_LEN} are needed",
            token.chars().count()
        ));
    }
    Ok(())
}

/// Constant-time comparison of two strings of any length.
fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

/// Serves requests until `max_requests` have been answered (`None` means forever). Each
/// connection carries one request and is closed after its response.
///
/// Support: Full.
pub fn serve_http(
    session: &mut Session,
    listener: &TcpListener,
    token: &str,
    start: Instant,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
    max_requests: Option<usize>,
) -> io::Result<()> {
    check_token(token).map_err(io::Error::other)?;
    for (count, stream) in listener.incoming().enumerate() {
        let mut stream = stream?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        // A bad connection ends with its own error and does not stop the server.
        let _ = handle(&mut stream, session, token, start, confirm);
        if max_requests.is_some_and(|max| count + 1 >= max) {
            break;
        }
    }
    Ok(())
}

fn handle(
    stream: &mut TcpStream,
    session: &mut Session,
    token: &str,
    start: Instant,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut header_bytes = request_line.len();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        header_bytes += read;
        if read == 0 || header_bytes > MAX_HEADER_BYTES {
            return respond(stream, 431, "headers too large", None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };

    // The body is read before any answer, so that a refused request still gets its response
    // instead of a reset connection.
    let declared = header("content-length").and_then(|v| v.parse::<usize>().ok());
    let body = match declared {
        Some(length) if length <= MAX_BODY_BYTES => {
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body)?;
            body
        }
        _ => Vec::new(),
    };

    let expected = format!("Bearer {token}");
    let authorised = header("authorization").is_some_and(|v| same_secret(v, &expected));
    if !authorised {
        return respond(stream, 401, "missing or wrong token", None);
    }
    let host_ok = header("host").is_some_and(|h| {
        let name = h.rsplit_once(':').map_or(h, |(name, _)| name);
        matches!(name, "127.0.0.1" | "localhost" | "[::1]")
    });
    if !host_ok {
        return respond(stream, 403, "the host must be the loopback address", None);
    }
    if path != "/mcp" {
        return respond(stream, 404, "not found", None);
    }
    if method != "POST" {
        return respond(stream, 405, "use POST", None);
    }
    let Some(length) = declared else {
        return respond(stream, 411, "a Content-Length is needed", None);
    };
    if length > MAX_BODY_BYTES {
        return respond(stream, 413, "the request is too large", None);
    }
    let Ok(text) = String::from_utf8(body) else {
        return respond(stream, 400, "the body is not UTF-8", None);
    };
    match session.handle_line(&text, start.elapsed(), confirm) {
        Some(reply) => respond(stream, 200, "", Some(&reply)),
        None => respond(stream, 202, "", None),
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    message: &str,
    body: Option<&str>,
) -> io::Result<()> {
    let text = match body {
        Some(b) => b.to_string(),
        None if message.is_empty() => String::new(),
        None => message.to_string(),
    };
    let content_type = if body.is_some() {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let www = if status == 401 {
        "WWW-Authenticate: Bearer\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{www}Connection: close\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

/// Confirms through a [`Confirm`] dialog. A convenience for callers that hold one.
///
/// Support: Full.
pub fn with_dialog(dialog: &mut dyn Confirm) -> impl FnMut(&str, &Value) -> bool + '_ {
    move |command, args| dialog.confirm(command, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentTool, DenyAll, Policy};
    use serde_json::json;
    use std::thread;
    use tpt_shell_ipc::schema::Schema;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn session() -> Session {
        Session::new(
            vec![AgentTool {
                command: "ping".into(),
                description: "ping".into(),
                args: Schema::Struct(&[]),
                handler: Box::new(|_| Ok(json!("pong"))),
            }],
            Policy::default(),
        )
    }

    fn request(addr: std::net::SocketAddr, raw: &str) -> String {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    fn post(token: &str, body: &str) -> String {
        format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn short_tokens_and_public_binds_are_refused() {
        assert!(bind_loopback(0, "short")
            .unwrap_err()
            .starts_with(CODE_HTTP));
        let listener = bind_loopback(0, TOKEN).unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }

    #[test]
    fn a_tool_call_over_http_needs_the_token() {
        let listener = bind_loopback(0, TOKEN).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ping","arguments":{}}}"#;
            let ok = request(addr, &post(TOKEN, body));
            let wrong = request(addr, &post("x".repeat(32).as_str(), body));
            let get = request(
                addr,
                &format!(
                    "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"
                ),
            );
            (ok, wrong, get)
        });
        let mut s = session();
        let mut deny = DenyAll;
        let mut ask = with_dialog(&mut deny);
        serve_http(&mut s, &listener, TOKEN, Instant::now(), &mut ask, Some(3)).unwrap();
        let (ok, wrong, get) = client.join().unwrap();
        assert!(ok.starts_with("HTTP/1.1 200 OK"), "{ok}");
        assert!(ok.contains("\"isError\":false"));
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        assert!(get.starts_with("HTTP/1.1 405"), "{get}");
        assert_eq!(
            s.log.len(),
            1,
            "only the authorised call reached the session"
        );
    }

    #[test]
    fn a_rebinding_host_header_is_refused() {
        let listener = bind_loopback(0, TOKEN).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = thread::spawn(move || {
            request(
                addr,
                &format!(
                    "POST /mcp HTTP/1.1\r\nHost: evil.example\r\nAuthorization: Bearer {TOKEN}\r\nContent-Length: 2\r\n\r\n{{}}"
                ),
            )
        });
        let mut s = session();
        let mut deny = DenyAll;
        let mut ask = with_dialog(&mut deny);
        serve_http(&mut s, &listener, TOKEN, Instant::now(), &mut ask, Some(1)).unwrap();
        assert!(client.join().unwrap().starts_with("HTTP/1.1 403"));
    }

    #[test]
    fn the_token_comparison_is_exact() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "a"));
    }
}
