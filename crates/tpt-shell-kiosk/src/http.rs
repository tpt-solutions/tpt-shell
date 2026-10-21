// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A small HTTP/1.1 server for the kiosk's local endpoints (spec §10.1). It is compiled only
//! when the `admin-endpoint` or `health-endpoint` feature is on, so a default build opens no
//! socket. It binds to loopback only and answers one request per connection.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// Largest request head read. Longer requests get `431`.
const MAX_HEAD: usize = 8 * 1024;

/// A parsed request. Only the parts the endpoints use are kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// The token from `Authorization: Bearer <token>` or `X-Tpt-Token`, if sent.
    pub token: Option<String>,
}

/// A response to write back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Response {
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Response {
            status,
            content_type: "application/json",
            body: body.into(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.into(),
        }
    }
}

/// Answers requests. Called on the server thread.
pub type Handler = Arc<dyn Fn(&Request) -> Response + Send + Sync>;

/// A running server. Dropping it stops the thread.
pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// The address the server is listening on (useful when port 0 was asked for).
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts a server on `addr`. Refuses any address that is not loopback.
pub fn serve(addr: SocketAddr, handler: Handler) -> io::Result<Server> {
    if !is_loopback(addr.ip()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{addr} is not a loopback address; the endpoint binds to 127.0.0.1 only"),
        ));
    }
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    let local = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("tpt-kiosk-http".into())
        .spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = handle(stream, &handler);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        })?;
    Ok(Server {
        addr: local,
        stop,
        thread: Some(thread),
    })
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
}

fn handle(mut stream: TcpStream, handler: &Handler) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let response = match read_head(&mut stream)? {
        Head::Complete(text) => match parse(&text) {
            Some(request) => handler(&request),
            None => Response::text(400, "bad request\n"),
        },
        Head::TooLarge => Response::text(431, "request head too large\n"),
    };
    write_response(&mut stream, &response)
}

enum Head {
    Complete(String),
    TooLarge,
}

fn read_head(stream: &mut TcpStream) -> io::Result<Head> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            return Ok(Head::Complete(String::from_utf8_lossy(&buf).into_owned()));
        }
        if buf.len() >= MAX_HEAD {
            return Ok(Head::TooLarge);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(Head::Complete(String::from_utf8_lossy(&buf).into_owned()));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Parses the request line and the two headers the endpoints use.
fn parse(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?;
    let version = parts.next()?;
    if !version.starts_with("HTTP/1.") {
        return None;
    }
    let path = target.split('?').next().unwrap_or("").to_string();
    let mut token = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            if let Some(bearer) = value.strip_prefix("Bearer ") {
                token = Some(bearer.trim().to_string());
            }
        } else if name.eq_ignore_ascii_case("x-tpt-token") {
            token = Some(value.to_string());
        }
    }
    Some(Request {
        method,
        path,
        token,
    })
}

fn write_response(stream: &mut TcpStream, response: &Response) -> io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(response.body.as_bytes())?;
    stream.flush()
}

/// Compares two strings in time that depends only on their lengths.
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn get(addr: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.write_all(request.as_bytes()).expect("send");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        out
    }

    #[test]
    fn requests_reach_the_handler_and_the_token_is_parsed() {
        let handler: Handler = Arc::new(|req: &Request| {
            Response::text(200, format!("{} {} {:?}", req.method, req.path, req.token))
        });
        let server = serve(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), handler).expect("serve");
        let reply = get(
            server.addr(),
            "GET /status?x=1 HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer abc\r\n\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.ends_with("GET /status Some(\"abc\")"), "{reply}");
    }

    #[test]
    fn a_non_loopback_address_is_refused() {
        let handler: Handler = Arc::new(|_: &Request| Response::text(200, ""));
        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0));
        assert_eq!(
            serve(addr, handler).err().map(|e| e.kind()),
            Some(io::ErrorKind::PermissionDenied)
        );
    }

    #[test]
    fn a_garbage_request_line_gets_400() {
        let handler: Handler = Arc::new(|_: &Request| Response::text(200, ""));
        let server = serve(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), handler).expect("serve");
        let reply = get(server.addr(), "NOT A REQUEST\r\n\r\n");
        assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    }

    #[test]
    fn constant_time_compare_checks_every_byte() {
        assert!(constant_time_eq("secret-token", "secret-token"));
        assert!(!constant_time_eq("secret-token", "secret-tokeN"));
        assert!(!constant_time_eq("secret", "secret-token"));
    }
}
