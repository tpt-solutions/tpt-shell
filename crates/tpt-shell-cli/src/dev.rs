// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell dev` (spec M3): serves an app's files on the loopback address and reports which
//! files change. It is started on purpose by the developer, so it is the only place the tool
//! opens a socket, and it refuses any address that is not loopback.
//!
//! Scope: GET and HEAD for files under one directory. There is no hot reload into a running
//! window yet; a change is printed so the developer can reload.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Modification times of every file under a directory, keyed by path relative to it.
pub type Snapshot = BTreeMap<PathBuf, SystemTime>;

/// Reads every file under `root`. Unreadable entries are skipped.
pub fn snapshot(root: &Path) -> Snapshot {
    let mut out = Snapshot::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(relative, modified);
            }
        }
    }
    out
}

/// The changes from `before` to `after`, as lines such as `added index.html`.
pub fn changes(before: &Snapshot, after: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    for (path, time) in after {
        match before.get(path) {
            None => out.push(format!("added {}", path.display())),
            Some(old) if old != time => out.push(format!("changed {}", path.display())),
            Some(_) => {}
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            out.push(format!("removed {}", path.display()));
        }
    }
    out
}

/// Watches `root` and prints each change, until the process ends.
pub fn watch(root: PathBuf, every: Duration) {
    std::thread::spawn(move || {
        let mut last = snapshot(&root);
        loop {
            std::thread::sleep(every);
            let now = snapshot(&root);
            for line in changes(&last, &now) {
                println!("{line}");
            }
            last = now;
        }
    });
}

/// Serves `root` on `addr`, one request at a time, until an error. `ready` gets the bound
/// address, so a caller that asked for port 0 learns the port.
pub fn serve(root: &Path, addr: SocketAddr, ready: impl FnOnce(SocketAddr)) -> Result<(), String> {
    if !is_loopback(addr.ip()) {
        return Err(format!(
            "{addr} is not a loopback address; the dev server only serves on this machine"
        ));
    }
    let listener = TcpListener::bind(addr).map_err(|e| format!("cannot listen on {addr}: {e}"))?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;
    ready(bound);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                // A request that fails part way only loses that one response.
                let _ = handle(root, stream);
            }
            Err(e) => return Err(format!("accept failed: {e}")),
        }
    }
    Ok(())
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
}

fn handle(root: &Path, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // Drain the headers, so the client has finished sending before we answer.
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            break;
        }
    }
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            405,
            "text/plain",
            b"method not allowed",
            method == "HEAD",
        );
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    match safe_file(root, path) {
        Some(file) => match std::fs::read(&file) {
            Ok(body) => respond(
                &mut stream,
                200,
                content_type(&file),
                &body,
                method == "HEAD",
            ),
            Err(_) => respond(
                &mut stream,
                404,
                "text/plain",
                b"not found",
                method == "HEAD",
            ),
        },
        None => respond(
            &mut stream,
            404,
            "text/plain",
            b"not found",
            method == "HEAD",
        ),
    }
}

/// The file a request path names under `root`, or `None` when the path would leave `root`.
pub fn safe_file(root: &Path, request_path: &str) -> Option<PathBuf> {
    let trimmed = request_path.trim_start_matches('/');
    let relative = if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    };
    let mut file = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.contains(['\\', ':']) {
                    return None;
                }
                file.push(part);
            }
            _ => return None,
        }
    }
    if file.is_file() {
        Some(file)
    } else {
        None
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    kind: &str,
    body: &[u8],
    head_only: bool,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Method Not Allowed",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-dev-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("js")).expect("dirs");
        std::fs::write(dir.join("index.html"), "<!doctype html><title>dev</title>").expect("index");
        std::fs::write(dir.join("js").join("app.js"), "1").expect("app");
        dir
    }

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("send");
        let mut out = String::new();
        stream.read_to_string(&mut out).expect("read");
        out
    }

    #[test]
    fn files_are_served_with_their_type_and_missing_ones_are_not_found() {
        let root = scratch("serve");
        let (tx, rx) = std::sync::mpsc::channel();
        let serving = root.clone();
        std::thread::spawn(move || {
            let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
            let _ = serve(&serving, addr, |bound| {
                let _ = tx.send(bound);
            });
        });
        let addr = rx.recv().expect("bound");

        let index = get(addr, "/");
        assert!(index.starts_with("HTTP/1.1 200 OK"), "{index}");
        assert!(index.contains("text/html") && index.contains("<title>dev</title>"));
        assert!(get(addr, "/js/app.js").contains("text/javascript"));
        assert!(get(addr, "/missing.css").starts_with("HTTP/1.1 404"));
        assert!(get(addr, "/../secret.txt").starts_with("HTTP/1.1 404"));
        assert!(get(addr, "/js/..%2Fsecret").starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_loopback_address_is_refused() {
        let root = scratch("refuse");
        let addr: SocketAddr = "0.0.0.0:0".parse().expect("addr");
        assert!(serve(&root, addr, |_| {}).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn changes_name_added_changed_and_removed_files() {
        let root = scratch("watch");
        let before = snapshot(&root);
        std::fs::write(root.join("new.txt"), "x").expect("add");
        std::fs::remove_file(root.join("js").join("app.js")).expect("remove");
        let after = snapshot(&root);
        let lines = changes(&before, &after);
        assert!(lines.contains(&"added new.txt".to_string()), "{lines:?}");
        assert!(
            lines.contains(&format!(
                "removed {}",
                Path::new("js").join("app.js").display()
            )),
            "{lines:?}"
        );

        let mut bumped = after.clone();
        let time = bumped.get(Path::new("index.html")).copied().expect("index");
        bumped.insert(PathBuf::from("index.html"), time + Duration::from_secs(5));
        assert_eq!(
            changes(&after, &bumped),
            vec!["changed index.html".to_string()]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_file_rejects_anything_outside_the_root() {
        let root = scratch("safe");
        assert!(safe_file(&root, "/index.html").is_some());
        assert!(safe_file(&root, "/").is_some());
        assert!(safe_file(&root, "/../x").is_none());
        assert!(safe_file(&root, "/C:/Windows/win.ini").is_none());
        assert!(safe_file(&root, "/a\\b").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
