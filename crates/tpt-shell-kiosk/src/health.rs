// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Optional local health endpoint (spec §10.1). Off by default: compiled only with the
//! `health-endpoint` feature. `GET /healthz` answers `200 ok` while the heartbeat file is fresh,
//! and `503` when it is missing or older than the limit. It binds to loopback only and needs no
//! token, because it reveals only whether the app is beating.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::http::{serve, Handler, Request, Response, Server};

/// Whether a heartbeat file is fresh: it exists and was written within `max_age`.
pub fn is_healthy(heartbeat: &Path, max_age: Duration) -> bool {
    let Ok(modified) = std::fs::metadata(heartbeat).and_then(|m| m.modified()) else {
        return false;
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age <= max_age,
        // A file written a little in the future (clock step) still counts as fresh.
        Err(ahead) => ahead.duration() <= max_age,
    }
}

/// Starts the health endpoint at `addr` (must be loopback), reading `heartbeat`.
pub fn start(addr: SocketAddr, heartbeat: PathBuf, max_age: Duration) -> std::io::Result<Server> {
    let handler: Handler = Arc::new(move |request: &Request| {
        if request.method != "GET" || request.path != "/healthz" {
            return Response::text(404, "not found\n");
        }
        if is_healthy(&heartbeat, max_age) {
            Response::text(200, "ok\n")
        } else {
            Response::text(503, "stale or missing heartbeat\n")
        }
    });
    serve(addr, handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpStream};

    #[test]
    fn a_missing_heartbeat_is_unhealthy_and_a_fresh_one_is_healthy() {
        let path = std::env::temp_dir().join(format!("tpt-kiosk-health-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(!is_healthy(&path, Duration::from_secs(30)));
        crate::heartbeat::beat(&path).expect("beat");
        assert!(is_healthy(&path, Duration::from_secs(30)));

        let server = start(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            path.clone(),
            Duration::from_secs(30),
        )
        .expect("start");
        let mut stream = TcpStream::connect(server.addr()).expect("connect");
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("send");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).expect("read");
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        let _ = std::fs::remove_file(&path);
    }
}
