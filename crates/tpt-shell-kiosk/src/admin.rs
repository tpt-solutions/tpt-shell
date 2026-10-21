// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Local admin endpoint (spec §10.1). Off by default: compiled only with the `admin-endpoint`
//! feature. It listens on loopback only and needs a token of at least [`MIN_TOKEN`] characters
//! on every request. Routes:
//!
//! - `GET /admin/status`: the status text from the app.
//! - `POST /admin/reset`: asks the app to return to its home page now.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::http::{constant_time_eq, serve, Handler, Request, Response, Server};

/// Shortest token accepted. A shorter token is refused at start-up.
pub const MIN_TOKEN: usize = 24;

/// What the admin endpoint can do, supplied by the app.
pub struct Actions {
    pub status: Arc<dyn Fn() -> String + Send + Sync>,
    pub reset: Arc<dyn Fn() + Send + Sync>,
}

/// Starts the admin endpoint at `addr` (must be loopback). Fails when the token is too short.
pub fn start(addr: SocketAddr, token: &str, actions: Actions) -> std::io::Result<Server> {
    if token.len() < MIN_TOKEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("the admin token must be at least {MIN_TOKEN} characters"),
        ));
    }
    let token = token.to_string();
    let handler: Handler = Arc::new(move |request: &Request| {
        let authorized = request
            .token
            .as_deref()
            .is_some_and(|sent| constant_time_eq(sent, &token));
        if !authorized {
            return Response::text(401, "unauthorized\n");
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/admin/status") => Response::text(200, (actions.status)()),
            ("POST", "/admin/reset") => {
                (actions.reset)();
                Response::text(200, "reset requested\n")
            }
            ("GET" | "POST", _) => Response::text(404, "not found\n"),
            _ => Response::text(405, "method not allowed\n"),
        }
    });
    serve(addr, handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpStream};
    use std::sync::atomic::{AtomicU32, Ordering};

    const TOKEN: &str = "a-token-that-is-long-enough";

    fn send(addr: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.write_all(request.as_bytes()).expect("send");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        out
    }

    fn actions(resets: Arc<AtomicU32>) -> Actions {
        Actions {
            status: Arc::new(|| "running\n".to_string()),
            reset: Arc::new(move || {
                resets.fetch_add(1, Ordering::SeqCst);
            }),
        }
    }

    #[test]
    fn requests_without_the_token_are_refused_and_do_nothing() {
        let resets = Arc::new(AtomicU32::new(0));
        let server = start(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            TOKEN,
            actions(resets.clone()),
        )
        .expect("start");
        let reply = send(
            server.addr(),
            "POST /admin/reset HTTP/1.1\r\nHost: x\r\n\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
        let wrong = send(
            server.addr(),
            "POST /admin/reset HTTP/1.1\r\nX-Tpt-Token: nope\r\n\r\n",
        );
        assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");
        assert_eq!(resets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_token_unlocks_status_and_reset() {
        let resets = Arc::new(AtomicU32::new(0));
        let server = start(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            TOKEN,
            actions(resets.clone()),
        )
        .expect("start");
        let status = send(
            server.addr(),
            &format!("GET /admin/status HTTP/1.1\r\nAuthorization: Bearer {TOKEN}\r\n\r\n"),
        );
        assert!(status.ends_with("running\n"), "{status}");
        let reset = send(
            server.addr(),
            &format!("POST /admin/reset HTTP/1.1\r\nX-Tpt-Token: {TOKEN}\r\n\r\n"),
        );
        assert!(reset.starts_with("HTTP/1.1 200"), "{reset}");
        assert_eq!(resets.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_short_token_is_refused_at_start() {
        let resets = Arc::new(AtomicU32::new(0));
        let err = start(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            "short",
            actions(resets),
        )
        .err()
        .expect("refused");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
