// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Windows checks. They drive WebView2 through the Windows backend's public API, the same
//! calls the backend's own runtime tests make. Each check gets a fresh backend and data folder.
//!
//! Needs `TPT_WEBVIEW2_LOADER` (the path to WebView2Loader.dll) and an interactive desktop.
//! Without them every check is reported as skipped.

use std::cell::RefCell;
use std::fmt::Display;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tpt_shell_conformance::{CheckResult, Outcome, RunReport, CHECKS};
use tpt_shell_core::backend::{Asset, MessageHandler, WebviewBackend, WindowBackend, WindowSpec};
use tpt_shell_core::{ColorScheme, Error, LogicalSize, Theme, ThemeBackend};
use tpt_shell_platform_windows::backend::{WindowId, WindowsBackend};
use tpt_shell_platform_windows::webview2::APP_ORIGIN;
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

const BACKEND: &str = "webview2";
const SKIP_REASON: &str = "needs TPT_WEBVIEW2_LOADER and an interactive desktop session";

/// Round trips in the IPC check. The page sends the next ping after each pong.
const ROUNDS: usize = 200;
/// Size of the large-blob check's payload.
const BLOB_BYTES: usize = 4 * 1024 * 1024;
/// Transfers in the blob check, one after another in one page. Their rates give the sustained
/// figure.
const BLOB_COUNT: usize = 8;
const WAIT: Duration = Duration::from_secs(60);

/// Files the app origin serves for one check: (path, content type, body).
type Pages = &'static [(&'static str, &'static str, &'static str)];

const START: &str = include_str!("../../pages/start.html");
const IPC_JS: &str = include_str!("../../pages/ipc.js");
const BLOB_JS: &str = include_str!("../../pages/blob.js");
const PROTOCOL_JS: &str = include_str!("../../pages/protocol.js");
const CSP_HTML: &str = include_str!("../../pages/csp.html");
const CSP_JS: &str = include_str!("../../pages/csp.js");
const NAV_JS: &str = include_str!("../../pages/nav.js");
const DPI_JS: &str = include_str!("../../pages/dpi.js");

const IPC_PAGES: Pages = &[
    ("index.html", "text/html", START),
    ("page.js", "text/javascript", IPC_JS),
];
const BLOB_PAGES: Pages = &[
    ("index.html", "text/html", START),
    ("page.js", "text/javascript", BLOB_JS),
];
const PROTOCOL_PAGES: Pages = &[
    ("index.html", "text/html", START),
    ("page.js", "text/javascript", PROTOCOL_JS),
];
const CSP_PAGES: Pages = &[
    ("index.html", "text/html", CSP_HTML),
    ("csp.js", "text/javascript", CSP_JS),
];
const NAV_PAGES: Pages = &[
    ("index.html", "text/html", START),
    ("page.js", "text/javascript", NAV_JS),
];
const DPI_PAGES: Pages = &[
    ("index.html", "text/html", START),
    ("page.js", "text/javascript", DPI_JS),
];

/// The page the theme check loads. It reports the colour scheme and the accent token.
const THEME_HTML: &str = "<!doctype html><title>x</title><script>document.addEventListener('DOMContentLoaded', function () { document.title = document.documentElement.dataset.colorScheme + '|' + document.documentElement.style.getPropertyValue('--tpt-accent').trim(); });</script>";

type CheckFn = fn(&Path, &Path) -> Result<(Outcome, String), String>;

/// Every check with its entry point. `needs_runtime` checks drive a window and are skipped when
/// there is no WebView2 loader or desktop. The others only report a pending backend feature.
const TABLE: &[(&str, bool, CheckFn)] = &[
    ("ipc-round-trip", true, ipc_round_trip),
    ("large-blobs", true, large_blobs),
    ("streams-backpressure", false, streams_backpressure),
    ("custom-protocol", true, custom_protocol),
    ("csp", true, csp),
    ("navigation-policy", true, navigation_policy),
    ("theme-injection", true, theme_injection),
    ("snapshot", false, snapshot),
    ("drag-regions", true, drag_regions),
    ("dpi", true, dpi),
];

/// Runs every check on WebView2 and returns the report for this OS.
pub fn run() -> RunReport {
    let runtime = runtime_test_loader();
    let loader = runtime.as_ref().map(|(path, _)| Path::new(path));

    let results = CHECKS
        .iter()
        .map(|name| {
            let Some((_, needs_runtime, check)) = TABLE.iter().find(|(n, _, _)| n == name) else {
                return result(
                    name,
                    Outcome::Pending("no check is written for this name".into()),
                    String::new(),
                );
            };
            let loader = match (loader, *needs_runtime) {
                (Some(path), _) => path,
                (None, true) => {
                    return result(name, Outcome::Skipped(SKIP_REASON.into()), String::new())
                }
                // A check that needs no backend does not look at the loader path.
                (None, false) => Path::new(""),
            };
            let dir =
                std::env::temp_dir().join(format!("tpt-conformance-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let (outcome, detail) = match check(loader, &dir) {
                Ok(found) => found,
                Err(message) => (Outcome::Fail(message), String::new()),
            };
            let _ = std::fs::remove_dir_all(&dir);
            result(name, outcome, detail)
        })
        .collect();

    RunReport {
        platform: std::env::consts::OS.into(),
        backend: BACKEND.into(),
        results,
    }
}

fn result(check: &str, outcome: Outcome, detail: String) -> CheckResult {
    CheckResult {
        check: check.into(),
        outcome,
        detail,
    }
}

fn err(e: impl Display) -> String {
    e.to_string()
}

fn backend(loader: &Path, dir: &Path) -> Result<WindowsBackend, String> {
    WindowsBackend::new(loader, dir).map_err(err)
}

/// Creates and shows a window whose app origin serves `pages`. The page is not loaded yet.
fn window(
    b: &mut WindowsBackend,
    pages: Pages,
    handler: Option<MessageHandler>,
) -> Result<WindowId, String> {
    let spec = WindowSpec {
        name: "main".into(),
        title: "conformance".into(),
        size: LogicalSize {
            width: 640.0,
            height: 480.0,
        },
        allow_navigation: Vec::new(),
    };
    let id = b.create(&spec).map_err(err)?;
    b.show(id).map_err(err)?;
    b.set_asset_source(
        id,
        Box::new(move |path| {
            pages
                .iter()
                .find(|(name, _, _)| *name == path)
                .map(|(_, content_type, body)| Asset {
                    content_type: (*content_type).into(),
                    body: body.as_bytes().to_vec(),
                })
        }),
    )
    .map_err(err)?;
    if let Some(handler) = handler {
        b.set_message_handler(id, handler).map_err(err)?;
    }
    Ok(id)
}

/// Creates a window with `pages` and navigates it to the app's start page.
fn open_app(
    b: &mut WindowsBackend,
    pages: Pages,
    handler: Option<MessageHandler>,
) -> Result<WindowId, String> {
    let id = window(b, pages, handler)?;
    b.navigate(id, &format!("{APP_ORIGIN}/index.html"))
        .map_err(err)?;
    Ok(id)
}

/// Pumps messages until the title satisfies `done` or the time runs out. Returns the last title.
fn wait_title(
    b: &WindowsBackend,
    id: WindowId,
    timeout: Duration,
    done: impl Fn(&str) -> bool,
) -> String {
    let mut title = String::new();
    pump_until(timeout, || {
        title = b.document_title(id).unwrap_or_default();
        done(&title)
    });
    title
}

fn ipc_round_trip(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let stamps: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
    let seen = stamps.clone();
    let handler: MessageHandler = Box::new(move |message: &str| {
        if message == "ping" {
            seen.borrow_mut().push(Instant::now());
            Some("pong".to_string())
        } else {
            None
        }
    });
    let id = open_app(&mut b, IPC_PAGES, Some(handler))?;
    let expected = format!("done:{ROUNDS}:pong");
    let title = wait_title(&b, id, WAIT, |t| t.starts_with("done:"));
    if title != expected {
        return Ok((
            Outcome::Fail(format!("title was {title:?}, expected {expected:?}")),
            String::new(),
        ));
    }

    let stamps = stamps.borrow();
    let mut gaps: Vec<f64> = stamps
        .windows(2)
        .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64() * 1000.0)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95 = gaps
        .get((gaps.len() * 95 / 100).min(gaps.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);
    let detail = format!(
        "{ROUNDS} round trips, p95 {p95:.2} ms between pings at the host (budget 5 ms, spec 13)"
    );
    Ok((Outcome::Pass, detail))
}

/// The blob check. Two parts, both must pass: the blob sent as a message, and the same size of
/// blob read through the app's blob URL, as a page does with a blob it was handed.
fn large_blobs(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let (message_outcome, message_detail) = large_blobs_message(loader, dir)?;
    let (fetch_outcome, fetch_detail) = large_blobs_fetch(loader, dir)?;
    let outcome = if matches!(message_outcome, Outcome::Pass) {
        fetch_outcome
    } else {
        message_outcome
    };
    Ok((outcome, format!("{message_detail}; {fetch_detail}")))
}

/// The blob sent as a message, eight times in a row.
fn large_blobs_message(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let blob = format!("{}z", "a".repeat(BLOB_BYTES - 1));
    // When the page asked for each blob. Each transfer is timed from one request to the next, so
    // page start-up (controller creation and navigation) is not counted. The last one ends when
    // the page reports.
    let asks: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
    let asks_seen = asks.clone();
    let handler: MessageHandler = Box::new(move |message: &str| {
        (message == "blob").then(|| {
            asks_seen.borrow_mut().push(Instant::now());
            blob.clone()
        })
    });
    let started = Instant::now();
    let id = open_app(&mut b, BLOB_PAGES, Some(handler))?;
    let expected = format!("blob:{BLOB_BYTES}:z:{BLOB_COUNT}");
    let title = wait_title(&b, id, WAIT, |t| t.starts_with("blob:"));
    let done = Instant::now();
    let total = done.duration_since(started).as_secs_f64();
    if title != expected {
        return Ok((
            Outcome::Fail(format!("title was {title:?}, expected {expected:?}")),
            String::new(),
        ));
    }
    let asks = asks.borrow();
    if asks.len() != BLOB_COUNT {
        return Ok((
            Outcome::Fail(format!(
                "the page asked {} times, expected {BLOB_COUNT}",
                asks.len()
            )),
            String::new(),
        ));
    }
    let mut ends: Vec<Instant> = asks.iter().skip(1).copied().collect();
    ends.push(done);
    let mut rates: Vec<f64> = asks
        .iter()
        .zip(&ends)
        .map(|(from, to)| {
            (BLOB_BYTES as f64 / 1_000_000.0) / to.duration_since(*from).as_secs_f64()
        })
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = rates[rates.len() / 2];
    let slowest = rates[0];
    let detail = format!(
        "{BLOB_COUNT} transfers of {} MiB, median {median:.1} MB/s, slowest {slowest:.1} MB/s (budget 50 MB/s sustained, spec 13); the whole check, page start-up included, took {:.0} ms",
        BLOB_BYTES / (1024 * 1024),
        total * 1000.0
    );
    Ok((Outcome::Pass, detail))
}

const FETCH_JS: &str = include_str!("../../pages/fetch.js");

/// The same size of blob, read eight times in a row through its blob URL. The blob is stored with
/// the facade's `put_blob`, and the page reads the URL it returns, so the path timed is the one a
/// real app's blobs take.
fn large_blobs_fetch(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let mut bytes = vec![b'a'; BLOB_BYTES];
    bytes[BLOB_BYTES - 1] = b'z';
    let url =
        tpt_shell::blob::put_blob("application/octet-stream", bytes).map_err(|e| e.to_string())?;
    let result = fetch_stored_blob(&mut b, &url);
    tpt_shell::blob::remove_blob(&url);
    result
}

fn fetch_stored_blob(b: &mut WindowsBackend, url: &str) -> Result<(Outcome, String), String> {
    let blob_path = url
        .strip_prefix(&format!("{APP_ORIGIN}/"))
        .ok_or_else(|| format!("blob URL {url} is not on the app origin"))?
        .to_string();
    let page_js = format!("var BLOB_PATH = {blob_path:?};\n{FETCH_JS}");
    let id = window(b, &[], None)?;
    // How long the host takes to look up each blob, in milliseconds: the facade's own lookup.
    let host_ms: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let host_seen = host_ms.clone();
    b.set_asset_source(
        id,
        Box::new(move |path| {
            if path.starts_with("__tpt/blob/") {
                let started = Instant::now();
                let asset = tpt_shell::__private::blob_asset_for_path(path);
                host_seen
                    .borrow_mut()
                    .push(started.elapsed().as_secs_f64() * 1000.0);
                return asset;
            }
            match path {
                "index.html" => Some(Asset {
                    content_type: "text/html".into(),
                    body: START.as_bytes().to_vec(),
                }),
                "page.js" => Some(Asset {
                    content_type: "text/javascript".into(),
                    body: page_js.as_bytes().to_vec(),
                }),
                _ => None,
            }
        }),
    )
    .map_err(err)?;
    b.navigate(id, &format!("{APP_ORIGIN}/index.html"))
        .map_err(err)?;
    let title = wait_title(b, id, WAIT, |t| t.starts_with("fetch:"));
    let expected_prefix = format!("fetch:{BLOB_BYTES}:z:{BLOB_COUNT}:");
    // The page reports the median of each whole read, then the median of its response alone.
    let times: Option<(f64, f64)> = title
        .strip_prefix(&expected_prefix)
        .and_then(|rest| rest.split_once(':'))
        .and_then(|(total, headers)| Some((total.parse().ok()?, headers.parse().ok()?)));
    let Some((total_ms, headers_ms)) = times else {
        return Ok((
            Outcome::Fail(format!(
                "title was {title:?}, expected {expected_prefix:?} followed by two times"
            )),
            String::new(),
        ));
    };
    let mb_per_s = (BLOB_BYTES as f64 / 1_000_000.0) / (total_ms / 1000.0);
    let host = host_ms.borrow();
    let mut host_sorted = host.clone();
    host_sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let host_median = host_sorted
        .get(host_sorted.len() / 2)
        .copied()
        .unwrap_or(0.0);
    let detail = format!(
        "fetched a stored blob through its URL: {BLOB_COUNT} reads of {} MiB, median {mb_per_s:.1} MB/s; median response {headers_ms:.1} ms of {total_ms:.1} ms, so the body took the rest; the facade's lookup took a median {host_median:.2} ms of that response",
        BLOB_BYTES / (1024 * 1024)
    );
    Ok((Outcome::Pass, detail))
}

fn streams_backpressure(_loader: &Path, _dir: &Path) -> Result<(Outcome, String), String> {
    Ok((
        Outcome::Pending(
            "the backend trait has no stream or backpressure method yet; the blob channel lives in the facade crate".into(),
        ),
        String::new(),
    ))
}

fn custom_protocol(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let id = open_app(&mut b, PROTOCOL_PAGES, None)?;
    let expected = format!("{APP_ORIGIN}|text/html|404");
    let title = wait_title(&b, id, Duration::from_secs(30), |t| t.contains('|'));
    if title != expected {
        return Ok((
            Outcome::Fail(format!("title was {title:?}, expected {expected:?}")),
            String::new(),
        ));
    }
    Ok((
        Outcome::Pass,
        "app origin served the start page as text/html; a missing file answered 404".into(),
    ))
}

fn csp(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let id = open_app(&mut b, CSP_PAGES, None)?;
    // The inline script runs first and sets the title. The external script then appends to it.
    let title = wait_title(&b, id, Duration::from_secs(30), |t| t.contains("external"));
    if title != "start|external" {
        return Ok((
            Outcome::Fail(format!(
                "title was {title:?}; expected \"start|external\" (inline script blocked, app script allowed)"
            )),
            String::new(),
        ));
    }
    Ok((
        Outcome::Pass,
        "inline script blocked, external script from the app origin ran".into(),
    ))
}

fn navigation_policy(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    // Requests from the backend are refused before anything loads.
    let probe = window(&mut b, START_ONLY, None)?;
    if b.navigate(probe, "https://example.com/").is_ok() {
        return Ok((
            Outcome::Fail("a remote URL was accepted".into()),
            String::new(),
        ));
    }
    if b.navigate(probe, "file:///C:/Windows/win.ini").is_ok() {
        return Ok((
            Outcome::Fail("a file URL was accepted".into()),
            String::new(),
        ));
    }

    // The page tries to leave by itself. After a few seconds it must still be on the app origin.
    let id = open_app(&mut b, NAV_PAGES, None)?;
    pump_until(Duration::from_secs(3), || false);
    let json = b
        .eval_json(id, "location.origin")
        .map_err(err)?
        .ok_or("the page did not answer the origin query")?;
    let origin: String = serde_json::from_str(&json).map_err(err)?;
    if origin != APP_ORIGIN {
        return Ok((
            Outcome::Fail(format!("the page left the app origin: now at {origin:?}")),
            String::new(),
        ));
    }
    Ok((
        Outcome::Pass,
        "remote and file URLs refused by the backend; the page's own navigation stayed on the app origin".into(),
    ))
}

const START_ONLY: Pages = &[("index.html", "text/html", START)];

fn theme_injection(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    b.set_theme(Theme {
        accent: "#123456".into(),
        ..Theme::default()
    })
    .map_err(err)?;
    let scheme = match ThemeBackend::color_scheme(&b).map_err(err)? {
        ColorScheme::Light => "light",
        ColorScheme::Dark => "dark",
    };
    let id = window(&mut b, START_ONLY, None)?;
    b.load_html(id, THEME_HTML).map_err(err)?;
    let expected = format!("{scheme}|#123456");
    let title = wait_title(&b, id, Duration::from_secs(30), |t| t == expected);
    if title != expected {
        return Ok((
            Outcome::Fail(format!("title was {title:?}, expected {expected:?}")),
            String::new(),
        ));
    }
    Ok((
        Outcome::Pass,
        format!("accent token and colour scheme ({scheme}) reached a new page"),
    ))
}

fn snapshot(_loader: &Path, _dir: &Path) -> Result<(Outcome, String), String> {
    Ok((
        Outcome::Pending(
            "the backend trait has no snapshot method yet (takeSnapshot is an M4 item for macOS)"
                .into(),
        ),
        String::new(),
    ))
}

fn drag_regions(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let id = window(&mut b, START_ONLY, None)?;
    Ok(match b.set_drag_regions(id, &[]) {
        Err(Error::Unsupported { feature }) => (
            Outcome::Pending(format!(
                "Windows does not implement {feature} yet; the trait default answers Unsupported"
            )),
            String::new(),
        ),
        Ok(()) => (
            Outcome::Pending(
                "set_drag_regions is accepted, but no drag behaviour check is written yet".into(),
            ),
            String::new(),
        ),
        Err(e) => (Outcome::Fail(err(e)), String::new()),
    })
}

fn dpi(loader: &Path, dir: &Path) -> Result<(Outcome, String), String> {
    let mut b = backend(loader, dir)?;
    let id = open_app(&mut b, DPI_PAGES, None)?;
    let title = wait_title(&b, id, Duration::from_secs(30), |t| t == "ready");
    if title != "ready" {
        return Ok((
            Outcome::Fail(format!("the page did not load: title {title:?}")),
            String::new(),
        ));
    }
    let ratio = b
        .eval_json(id, "window.devicePixelRatio")
        .map_err(err)?
        .unwrap_or_else(|| "unknown".into());
    Ok((
        Outcome::Pending(
            "the backend has no public scale-factor query, so the check cannot compare it yet"
                .into(),
        ),
        format!("the page reports devicePixelRatio {ratio} in this session"),
    ))
}

/// The probe script, served from the app origin. Inline scripts are blocked there by the app's
/// content security policy (the csp check), so the probe loads an external script.
const PROBE_JS: &str = include_str!("../../pages/probe.js");

/// Cold start, timed on the host. Not a check in the report, so it is ignored by default. Run with
/// `cargo test --manifest-path conformance/Cargo.toml --release --test conformance -- --ignored
/// --nocapture cold_start`. Prints three times: the window returned from `create` (which waits for
/// the WebView2 controller), the navigation started, and the page being parsed.
#[test]
#[ignore = "a measurement, run on request"]
fn cold_start_probe() {
    let Some(loader) = runtime_test_loader().map(|(path, _)| path) else {
        eprintln!("skipped: no WebView2 loader");
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-cold-start-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let loader = Path::new(&loader);
    let started = Instant::now();
    let mut b = backend(loader, &dir).expect("backend");
    let probe_pages: Pages = &[
        ("index.html", "text/html", START),
        ("page.js", "text/javascript", PROBE_JS),
    ];
    let id = window(&mut b, probe_pages, None).expect("window");
    let created = started.elapsed();
    b.navigate(id, &format!("{APP_ORIGIN}/index.html"))
        .expect("navigate");
    let navigated = started.elapsed();
    let title = wait_title(&b, id, Duration::from_secs(20), |t| t == "parsed");
    let framed = started.elapsed();
    println!(
        "cold start, title {title:?}: window created {:.0} ms (controller included), navigation started {:.0} ms, page parsed {:.0} ms",
        created.as_secs_f64() * 1000.0,
        navigated.as_secs_f64() * 1000.0,
        framed.as_secs_f64() * 1000.0
    );
    drop(b);
    let _ = std::fs::remove_dir_all(&dir);
}
