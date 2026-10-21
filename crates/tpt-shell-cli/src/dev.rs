// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell dev` (spec M3): serves an app's files on the loopback address and reports which
//! files change. It is started on purpose by the developer, so it is the only place the tool
//! opens a socket, and it refuses any address that is not loopback.
//!
//! Scope: GET and HEAD for files under one directory. Reloading is done by the page, not the
//! process: each served HTML page polls `/__tpt/version` and reloads itself when a change is
//! seen, so the Rust side keeps its state. After a reload the page asks `/__tpt/session` for
//! the last recorded session (`.tpt-dev/last.tptreplay`) and gets it as a `tpt-shell-dev-replay`
//! event. The replay player itself is M8 work; this is the hook.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Modification times of every file under a directory, keyed by path relative to it.
pub type Snapshot = BTreeMap<PathBuf, SystemTime>;

/// Where the last recorded session is kept, relative to the served directory.
const SESSION_FILE: &str = ".tpt-dev/last.tptreplay";

/// The version endpoint the reload script polls.
const VERSION_PATH: &str = "/__tpt/version";
/// The reload script that every served HTML page gets.
const SCRIPT_PATH: &str = "/__tpt/reload.js";
/// The last recorded session.
const SESSION_PATH: &str = "/__tpt/session";

/// Counts the change batches seen by the watcher. Pages compare it with the value they loaded
/// with, so a change reloads them without restarting the process.
#[derive(Debug, Default)]
pub struct Reloads {
    version: AtomicU64,
}

impl Reloads {
    /// The current version. It goes up by one for each batch of changes.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::SeqCst);
    }
}

/// The script added to every served HTML page. It reloads the page when the version changes.
/// After a reload it hands the last recorded session to the page as a `tpt-shell-dev-replay`
/// event, so the app or a replay player can pick it up.
const RELOAD_SCRIPT: &str = r#"(function () {
  var key = "tpt-dev-reloaded";
  var seen = null;
  function takeFlag() {
    try {
      var value = sessionStorage.getItem(key);
      sessionStorage.removeItem(key);
      return value;
    } catch (e) {
      return null;
    }
  }
  function setFlag() {
    try {
      sessionStorage.setItem(key, "1");
    } catch (e) {}
  }
  function replayLast() {
    if (takeFlag() !== "1") return;
    fetch("/__tpt/session", { cache: "no-store" })
      .then(function (r) { return r.ok ? r.text() : null; })
      .then(function (text) {
        if (text !== null) {
          window.dispatchEvent(new CustomEvent("tpt-shell-dev-replay", { detail: text }));
        }
      })
      .catch(function () {});
  }
  function poll() {
    fetch("/__tpt/version", { cache: "no-store" })
      .then(function (r) { return r.text(); })
      .then(function (version) {
        if (seen === null) {
          seen = version;
        } else if (version !== seen) {
          setFlag();
          location.reload();
        }
      })
      .catch(function () {});
  }
  replayLast();
  poll();
  setInterval(poll, 500);
})();
"#;

/// Inserts the reload script into an HTML page, before `</body>` when there is one.
pub fn inject_reload_script(html: &str) -> String {
    let tag = format!("<script src=\"{SCRIPT_PATH}\"></script>");
    let lower = html.to_ascii_lowercase();
    match lower.rfind("</body>") {
        Some(at) => format!("{}{tag}{}", &html[..at], &html[at..]),
        None => format!("{html}{tag}"),
    }
}

/// The last recorded session, if one exists and reads cleanly in lenient mode. A file the
/// reader rejects is not replayed.
pub fn last_session(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(SESSION_FILE)).ok()?;
    tpt_shell_replay::read(&text, tpt_shell_replay::Mode::Lenient).ok()?;
    Some(text)
}

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

/// Watches `root`, prints each change and bumps `reloads` once per batch, until the process
/// ends.
pub fn watch(root: PathBuf, every: Duration, reloads: Arc<Reloads>) {
    std::thread::spawn(move || {
        let mut last = snapshot(&root);
        loop {
            std::thread::sleep(every);
            let now = snapshot(&root);
            let lines = changes(&last, &now);
            for line in &lines {
                println!("{line}");
            }
            if !lines.is_empty() {
                reloads.bump();
            }
            last = now;
        }
    });
}

/// Serves `root` on `addr`, one request at a time, until an error. `reloads` is the counter the
/// watcher bumps. `ready` gets the bound address, so a caller that asked for port 0 learns the
/// port.
pub fn serve(
    root: &Path,
    addr: SocketAddr,
    reloads: &Reloads,
    ready: impl FnOnce(SocketAddr),
) -> Result<(), String> {
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
                let _ = handle(root, reloads, stream);
            }
            Err(e) => return Err(format!("accept failed: {e}")),
        }
    }
    Ok(())
}

fn is_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
}

fn handle(root: &Path, reloads: &Reloads, mut stream: TcpStream) -> std::io::Result<()> {
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
    let head_only = method == "HEAD";
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            405,
            "text/plain",
            b"method not allowed",
            head_only,
        );
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    match path {
        VERSION_PATH => {
            let body = reloads.version().to_string();
            return respond(&mut stream, 200, "text/plain", body.as_bytes(), head_only);
        }
        SCRIPT_PATH => {
            let kind = "text/javascript; charset=utf-8";
            return respond(&mut stream, 200, kind, RELOAD_SCRIPT.as_bytes(), head_only);
        }
        SESSION_PATH => {
            return match last_session(root) {
                Some(text) => respond(
                    &mut stream,
                    200,
                    "application/x-ndjson; charset=utf-8",
                    text.as_bytes(),
                    head_only,
                ),
                None => respond(&mut stream, 404, "text/plain", b"not found", head_only),
            };
        }
        _ => {}
    }
    match safe_file(root, path) {
        Some(file) => match std::fs::read(&file) {
            Ok(body) => {
                let kind = content_type(&file);
                if kind.starts_with("text/html") {
                    let page = inject_reload_script(&String::from_utf8_lossy(&body));
                    respond(&mut stream, 200, kind, page.as_bytes(), head_only)
                } else {
                    respond(&mut stream, 200, kind, &body, head_only)
                }
            }
            Err(_) => respond(&mut stream, 404, "text/plain", b"not found", head_only),
        },
        None => respond(&mut stream, 404, "text/plain", b"not found", head_only),
    }
}

/// The file a request path names under `root`, or `None` when the path would leave `root` or
/// names a hidden file or folder (such as `.tpt-dev`, which holds recordings).
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
                if text.contains(['\\', ':']) || text.starts_with('.') {
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
    use std::io::{ErrorKind, Read};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-dev-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("js")).expect("dirs");
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>dev</title><body>hi</body>",
        )
        .expect("index");
        std::fs::write(dir.join("js").join("app.js"), "1").expect("app");
        dir
    }

    /// Starts a server on a free port for `root`. The thread lives until the test process ends.
    fn start(root: PathBuf, reloads: Arc<Reloads>) -> SocketAddr {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let addr: SocketAddr = "127.0.0.1:0".parse().expect("addr");
            let _ = serve(&root, addr, &reloads, |bound| {
                let _ = tx.send(bound);
            });
        });
        rx.recv().expect("bound")
    }

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("send");
        let mut out = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                // Windows can report a reset instead of a clean end once the server has closed
                // after answering. Keep what arrived; the assertions check the status line.
                Err(e) if e.kind() == ErrorKind::ConnectionReset && !out.is_empty() => break,
                Err(e) => panic!("read: {e:?}"),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn files_are_served_with_their_type_and_missing_ones_are_not_found() {
        let root = scratch("serve");
        let addr = start(root.clone(), Arc::new(Reloads::default()));

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
    fn pages_get_the_reload_script_and_the_version_moves_on_change() {
        let root = scratch("reload");
        let reloads = Arc::new(Reloads::default());
        let addr = start(root.clone(), Arc::clone(&reloads));

        let index = get(addr, "/");
        assert!(index.contains(SCRIPT_PATH), "{index}");
        assert!(index.find(SCRIPT_PATH) < index.find("</body>"));
        assert!(get(addr, "/__tpt/version").ends_with("\r\n\r\n0"));
        reloads.bump();
        assert!(get(addr, "/__tpt/version").ends_with("\r\n\r\n1"));
        assert!(get(addr, SCRIPT_PATH).contains("location.reload"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_last_session_is_served_only_when_it_reads_cleanly() {
        let root = scratch("session");
        let addr = start(root.clone(), Arc::new(Reloads::default()));
        assert!(get(addr, SESSION_PATH).starts_with("HTTP/1.1 404"));

        std::fs::create_dir_all(root.join(".tpt-dev")).expect("dir");
        let mut recorder = tpt_shell_replay::Recorder::new(tpt_shell_replay::Header {
            app: "demo".into(),
            platform: "windows".into(),
        });
        recorder
            .record(1, "ipc_request", serde_json::json!({ "command": "greet" }))
            .expect("record");
        std::fs::write(root.join(SESSION_FILE), recorder.finish()).expect("write");
        assert!(get(addr, SESSION_PATH).contains("\"greet\""));
        // The recording is hidden from the static file route.
        assert!(get(addr, "/.tpt-dev/last.tptreplay").starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_damaged_session_is_not_replayed() {
        let root = scratch("damaged");
        std::fs::create_dir_all(root.join(".tpt-dev")).expect("dir");
        std::fs::write(root.join(SESSION_FILE), "not a recording\n").expect("write");
        assert_eq!(last_session(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_non_loopback_address_is_refused() {
        let root = scratch("refuse");
        let addr: SocketAddr = "0.0.0.0:0".parse().expect("addr");
        assert!(serve(&root, addr, &Reloads::default(), |_| {}).is_err());
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
        assert!(safe_file(&root, "/.env").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
