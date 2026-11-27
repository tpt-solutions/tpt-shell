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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
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

/// A refusal: the HTTP status and the short reason the body carries.
type Refusal = (u16, &'static str);

/// One request as the socket delivered it, before any check.
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    declared: Option<usize>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Reads one request: the request line, the headers and, when it is small enough, the body. The
/// body is read before any answer, so that a refused request still gets its response instead of
/// a reset connection. An oversized header block is a refusal, not an error.
fn read_request(stream: &mut TcpStream) -> io::Result<Result<Request, Refusal>> {
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
            return Ok(Err((431, "headers too large")));
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let declared = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok());
    let body = match declared {
        Some(length) if length <= MAX_BODY_BYTES => {
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body)?;
            body
        }
        _ => Vec::new(),
    };
    Ok(Ok(Request {
        method,
        path,
        headers,
        declared,
        body,
    }))
}

/// The checks a request must pass before the session sees it, in the order they are made: the
/// token, the host, the path, the method, the length and the text. On success, the body text.
fn check(request: &Request, token: &str) -> Result<String, Refusal> {
    let expected = format!("Bearer {token}");
    let authorised = request
        .header("authorization")
        .is_some_and(|v| same_secret(v, &expected));
    if !authorised {
        return Err((401, "missing or wrong token"));
    }
    let host_ok = request.header("host").is_some_and(|h| {
        let name = h.rsplit_once(':').map_or(h, |(name, _)| name);
        matches!(name, "127.0.0.1" | "localhost" | "[::1]")
    });
    if !host_ok {
        return Err((403, "the host must be the loopback address"));
    }
    if request.path != "/mcp" {
        return Err((404, "not found"));
    }
    if request.method != "POST" {
        return Err((405, "use POST"));
    }
    let Some(length) = request.declared else {
        return Err((411, "a Content-Length is needed"));
    };
    if length > MAX_BODY_BYTES {
        return Err((413, "the request is too large"));
    }
    String::from_utf8(request.body.clone()).map_err(|_| (400, "the body is not UTF-8"))
}

fn handle(
    stream: &mut TcpStream,
    session: &mut Session,
    token: &str,
    start: Instant,
    confirm: &mut dyn FnMut(&str, &Value) -> bool,
) -> io::Result<()> {
    let request = match read_request(stream)? {
        Ok(request) => request,
        Err((status, message)) => return respond(stream, status, message, None),
    };
    let text = match check(&request, token) {
        Ok(text) => text,
        Err((status, message)) => return respond(stream, status, message, None),
    };
    match session.handle_line(&text, start.elapsed(), confirm) {
        Some(reply) => respond(stream, 200, "", Some(&reply)),
        None => respond(stream, 202, "", None),
    }
}

/// How many connections may be open at once. A connection past this gets 503 at once.
pub const MAX_CONNECTIONS: usize = 8;

/// How long a connection waits for the UI thread to answer its request.
const ANSWER_WAIT: Duration = Duration::from_secs(10);

/// A request that passed the checks, waiting for the session on the UI thread.
struct Pending {
    body: String,
    reply: mpsc::SyncSender<Option<String>>,
}

/// The UI-thread end of the HTTP transport. The session is not `Send`, so the sockets are served
/// on background threads and only checked requests come here. Call [`Bridge::answer_pending`]
/// from the UI thread's tick.
///
/// Support: Full.
pub struct Bridge {
    pending: mpsc::Receiver<Pending>,
}

impl Bridge {
    /// Answers every request that is waiting. Returns how many were answered. A request that gets
    /// no answer within the wait is refused by its connection with 503.
    ///
    /// Support: Full.
    pub fn answer_pending(
        &self,
        session: &mut Session,
        start: Instant,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    ) -> usize {
        let mut answered = 0;
        while let Ok(pending) = self.pending.try_recv() {
            let reply = session.handle_line(&pending.body, start.elapsed(), confirm);
            // The connection may have given up; a reply nobody reads is dropped.
            let _ = pending.reply.send(reply);
            answered += 1;
        }
        answered
    }
}

/// Starts accepting connections on `listener` on a background thread, and returns the bridge the
/// UI thread answers from. The token is checked here, so a short one is refused before any socket
/// is served.
///
/// Support: Full.
pub fn spawn_bridge(listener: TcpListener, token: &str) -> Result<Bridge, String> {
    check_token(token)?;
    let token = token.to_string();
    let (tx, rx) = mpsc::channel::<Pending>();
    let open = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                let _ = respond(&mut stream, 503, "too many connections", None);
                continue;
            }
            open.fetch_add(1, Ordering::SeqCst);
            let (tx, token, open) = (tx.clone(), token.clone(), open.clone());
            thread::spawn(move || {
                let _ = serve_connection(stream, &token, &tx);
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(Bridge { pending: rx })
}

/// Serves one connection: checks the request here, then waits for the UI thread's answer.
fn serve_connection(
    mut stream: TcpStream,
    token: &str,
    tx: &mpsc::Sender<Pending>,
) -> io::Result<()> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let request = match read_request(&mut stream)? {
        Ok(request) => request,
        Err((status, message)) => return respond(&mut stream, status, message, None),
    };
    let text = match check(&request, token) {
        Ok(text) => text,
        Err((status, message)) => return respond(&mut stream, status, message, None),
    };
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    if tx
        .send(Pending {
            body: text,
            reply: reply_tx,
        })
        .is_err()
    {
        return respond(&mut stream, 503, "the app is not running", None);
    }
    match reply_rx.recv_timeout(ANSWER_WAIT) {
        Ok(Some(reply)) => respond(&mut stream, 200, "", Some(&reply)),
        Ok(None) => respond(&mut stream, 202, "", None),
        Err(_) => respond(&mut stream, 503, "the app did not answer in time", None),
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
        503 => "Service Unavailable",
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
    fn the_bridge_answers_on_the_ui_thread_and_refuses_bad_requests_without_it() {
        let listener = bind_loopback(0, TOKEN).unwrap();
        let addr = listener.local_addr().unwrap();
        let bridge = spawn_bridge(listener, TOKEN).unwrap();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ping","arguments":{}}}"#;
        let client = thread::spawn(move || {
            let ok = request(addr, &post(TOKEN, body));
            let wrong = request(addr, &post("x".repeat(32).as_str(), body));
            (ok, wrong)
        });
        let mut s = session();
        let mut deny = DenyAll;
        let mut ask = with_dialog(&mut deny);
        // The UI thread answers the waiting request until the client has both replies.
        for _ in 0..1000 {
            bridge.answer_pending(&mut s, Instant::now(), &mut ask);
            if client.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let (ok, wrong) = client.join().unwrap();
        assert!(ok.starts_with("HTTP/1.1 200 OK"), "{ok}");
        assert!(ok.contains("\"isError\":false"), "{ok}");
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        assert_eq!(
            s.log.len(),
            1,
            "only the authorised call reached the session"
        );
    }

    #[test]
    fn a_bridge_refuses_a_short_token_before_serving_anything() {
        let listener = bind_loopback(0, TOKEN).unwrap();
        assert!(spawn_bridge(listener, "short").is_err());
    }

    #[test]
    fn the_token_comparison_is_exact() {
        assert!(same_secret("abc", "abc"));
        assert!(!same_secret("abc", "abd"));
        assert!(!same_secret("abc", "abcd"));
        assert!(!same_secret("", "a"));
    }
}
