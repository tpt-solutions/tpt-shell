// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Linux checks. They drive GTK and WebKitGTK through the Linux backend's public API, the same
//! calls the backend's own runtime tests make. Each check gets a fresh backend.
//!
//! The backend has no page-title query yet, so the pages in `pages/linux/` report their results
//! through the message channel (`report:...`). The checks that cannot be run without a backend
//! feature are pending, with the reason in the report.
//!
//! Needs a display (`DISPLAY` or `WAYLAND_DISPLAY`) and GTK 3 or 4 with WebKitGTK at run time.
//! Without them every check is reported as skipped, or as failed when `TPT_LINUX_STRICT` is set.

use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tpt_shell_conformance::{CheckResult, Outcome, RunReport, CHECKS};
use tpt_shell_core::backend::{Asset, MessageHandler, WebviewBackend, WindowBackend, WindowSpec};
use tpt_shell_core::{Error, LogicalSize};
use tpt_shell_platform_linux::adapter::LinuxBackend;
use tpt_shell_platform_linux::gtk3::WindowId;

const BACKEND: &str = "webkitgtk";
/// The origin the Linux backend serves app pages from (see `gtk3::route`).
const APP_ORIGIN: &str = "app://tpt-app";

/// Round trips in the IPC check. The page sends the next ping after each pong.
const ROUNDS: usize = 200;
/// Size of the large-blob check's payload.
const BLOB_BYTES: usize = 4 * 1024 * 1024;
/// Transfers in the blob check, one after another in one page.
const BLOB_COUNT: usize = 8;
const WAIT: Duration = Duration::from_secs(60);
/// How long a check waits after its last message, to catch a message that should not come.
const QUIET: Duration = Duration::from_secs(1);

/// Files the app origin serves for one check: (path, content type, body).
type Pages = &'static [(&'static str, &'static str, &'static str)];
/// A check's answer, or the outcome of a step that could not finish (for example no display).
type Step<T> = Result<T, Outcome>;
type Found = (Outcome, String);
type CheckFn = fn() -> Step<Found>;

const START: &str = include_str!("../../pages/start.html");
const IPC_JS: &str = include_str!("../../pages/ipc.js");
const BLOB_JS: &str = include_str!("../../pages/linux/blob.js");
const PROTOCOL_JS: &str = include_str!("../../pages/linux/protocol.js");
const CSP_HTML: &str = include_str!("../../pages/linux/csp.html");
const CSP_JS: &str = include_str!("../../pages/linux/csp.js");
const NAV_JS: &str = include_str!("../../pages/linux/nav.js");

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
const START_ONLY: Pages = &[("index.html", "text/html", START)];

/// A page that reports once it has loaded, for the snapshot check.
const LOADED_HTML: &str = r#"<!doctype html><body style="background:#336"><script>window.chrome.webview.postMessage("report:loaded");</script></body>"#;

/// Every check with its entry point, by name.
const TABLE: &[(&str, CheckFn)] = &[
    ("ipc-round-trip", ipc_round_trip),
    ("large-blobs", large_blobs),
    ("streams-backpressure", streams_backpressure),
    ("custom-protocol", custom_protocol),
    ("csp", csp),
    ("navigation-policy", navigation_policy),
    ("theme-injection", theme_injection),
    ("snapshot", snapshot),
    ("drag-regions", drag_regions),
    ("dpi", dpi),
];

/// Runs every check on the Linux backend and returns the report for this OS.
pub fn run() -> RunReport {
    let results = CHECKS
        .iter()
        .map(|name| {
            let Some((_, check)) = TABLE.iter().find(|(n, _)| n == name) else {
                return result(
                    name,
                    Outcome::Pending("no check is written for this name".into()),
                    String::new(),
                );
            };
            let (outcome, detail) = match check() {
                Ok(found) => found,
                Err(outcome) => (outcome, String::new()),
            };
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

fn fail(e: impl Display) -> Outcome {
    Outcome::Fail(e.to_string())
}

fn pending(reason: impl Into<String>) -> Step<Found> {
    Ok((Outcome::Pending(reason.into()), String::new()))
}

fn strict() -> bool {
    std::env::var_os("TPT_LINUX_STRICT").is_some_and(|v| !v.is_empty())
}

/// A fresh backend. Without a display or the toolkit libraries the check is skipped, or failed
/// under `TPT_LINUX_STRICT` (the CI job sets it, so a missing toolkit cannot pass silently).
fn backend() -> Step<LinuxBackend> {
    LinuxBackend::new().map_err(|e| {
        if strict() {
            Outcome::Fail(e.to_string())
        } else {
            Outcome::Skipped(e.to_string())
        }
    })
}

/// Creates a window whose app origin serves `pages` and shows it. The page is not loaded yet.
fn window(b: &mut LinuxBackend, pages: Pages, handler: Option<MessageHandler>) -> Step<WindowId> {
    let spec = WindowSpec {
        name: "main".into(),
        title: "conformance".into(),
        size: LogicalSize {
            width: 640.0,
            height: 480.0,
        },
        allow_navigation: Vec::new(),
    };
    let id = b.create(&spec).map_err(fail)?;
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
    .map_err(fail)?;
    if let Some(handler) = handler {
        b.set_message_handler(id, handler).map_err(fail)?;
    }
    b.show(id).map_err(fail)?;
    Ok(id)
}

/// Creates a window with `pages` and navigates it to the app's start page.
fn open_app(b: &mut LinuxBackend, pages: Pages, handler: Option<MessageHandler>) -> Step<WindowId> {
    let id = window(b, pages, handler)?;
    b.navigate(id, &format!("{APP_ORIGIN}/index.html"))
        .map_err(fail)?;
    Ok(id)
}

/// The reports the pages post (`report:...`), in the order they arrived.
type Reports = Rc<RefCell<Vec<String>>>;

/// A message handler that only records what the page posts.
fn report_sink() -> (Reports, MessageHandler) {
    let log: Reports = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    let handler: MessageHandler = Box::new(move |message: &str| {
        seen.borrow_mut().push(message.to_string());
        None
    });
    (log, handler)
}

/// Runs the main loop until a recorded message satisfies `found`, or the time runs out. Returns
/// the first such message.
fn wait_for(
    b: &mut LinuxBackend,
    log: &Reports,
    timeout: Duration,
    found: impl Fn(&str) -> bool,
) -> Option<String> {
    let _ = b.run_until(|| log.borrow().iter().any(|m| found(m)), timeout);
    log.borrow().iter().find(|m| found(m)).cloned()
}

fn ipc_round_trip() -> Step<Found> {
    let mut b = backend()?;
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
    open_app(&mut b, IPC_PAGES, Some(handler))?;
    b.run_until(|| stamps.borrow().len() >= ROUNDS, WAIT);
    let _ = b.run_until(|| false, QUIET);
    let stamps = stamps.borrow();
    if stamps.len() != ROUNDS {
        return Err(Outcome::Fail(format!(
            "the host saw {} pings, expected {ROUNDS}",
            stamps.len()
        )));
    }
    let mut gaps: Vec<f64> = stamps
        .windows(2)
        .map(|pair| pair[1].duration_since(pair[0]).as_secs_f64() * 1000.0)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95 = gaps
        .get((gaps.len() * 95 / 100).min(gaps.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0);
    Ok((
        Outcome::Pass,
        format!(
            "{ROUNDS} round trips, p95 {p95:.2} ms between pings at the host (budget 5 ms, spec 13)"
        ),
    ))
}

/// The blob sent as a message, eight times in a row. The blob-URL half is not ported yet, so a
/// passing message half is reported as pending, not as a pass.
fn large_blobs() -> Step<Found> {
    let mut b = backend()?;
    let blob = format!("{}z", "a".repeat(BLOB_BYTES - 1));
    // When the page asked for each blob.
    let asks: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
    let asks_seen = asks.clone();
    let handler: MessageHandler = Box::new(move |message: &str| {
        (message == "blob").then(|| {
            asks_seen.borrow_mut().push(Instant::now());
            blob.clone()
        })
    });
    open_app(&mut b, BLOB_PAGES, Some(handler))?;
    b.run_until(|| asks.borrow().len() >= BLOB_COUNT, WAIT);
    let _ = b.run_until(|| false, QUIET);
    let asks = asks.borrow();
    if asks.len() != BLOB_COUNT {
        return Err(Outcome::Fail(format!(
            "the page asked {} times, expected {BLOB_COUNT}",
            asks.len()
        )));
    }
    // Each transfer runs from one request to the next.
    let mut rates: Vec<f64> = asks
        .windows(2)
        .map(|pair| {
            (BLOB_BYTES as f64 / 1_000_000.0) / pair[1].duration_since(pair[0]).as_secs_f64()
        })
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = rates[rates.len() / 2];
    let slowest = rates[0];
    pending(format!(
        "the message half passed: {} transfers of {} MiB, median {median:.1} MB/s, slowest {slowest:.1} MB/s (budget 50 MB/s, spec 13). The blob-URL half is not ported to the Linux backend yet",
        rates.len(),
        BLOB_BYTES / (1024 * 1024)
    ))
}

fn streams_backpressure() -> Step<Found> {
    pending("the backend trait has no stream or backpressure method yet; the blob channel lives in the facade crate")
}

fn custom_protocol() -> Step<Found> {
    let mut b = backend()?;
    let (log, handler) = report_sink();
    open_app(&mut b, PROTOCOL_PAGES, Some(handler))?;
    let expected = format!("report:{APP_ORIGIN}|text/html|404");
    match wait_for(&mut b, &log, Duration::from_secs(30), |m| {
        m.starts_with("report:")
    }) {
        Some(got) if got == expected => Ok((
            Outcome::Pass,
            "app origin served the start page as text/html; a missing file answered 404".into(),
        )),
        Some(got) => Err(Outcome::Fail(format!(
            "report was {got:?}, expected {expected:?}"
        ))),
        None => Err(Outcome::Fail("the page did not report in 30 s".into())),
    }
}

fn csp() -> Step<Found> {
    let mut b = backend()?;
    let (log, handler) = report_sink();
    open_app(&mut b, CSP_PAGES, Some(handler))?;
    // The inline script sets a flag. The external script reports it, so a blocked inline script
    // shows as "inline-blocked" and the external script still runs.
    let expected = "report:inline-blocked|external";
    match wait_for(&mut b, &log, Duration::from_secs(30), |m| {
        m.starts_with("report:")
    }) {
        Some(got) if got == expected => Ok((
            Outcome::Pass,
            "inline script blocked, external script from the app origin ran".into(),
        )),
        Some(got) => Err(Outcome::Fail(format!(
            "report was {got:?}; expected {expected:?} (inline script blocked, app script allowed)"
        ))),
        None => Err(Outcome::Fail("the page did not report in 30 s".into())),
    }
}

fn navigation_policy() -> Step<Found> {
    let mut b = backend()?;
    // Requests from the backend are refused before anything loads.
    let probe = window(&mut b, START_ONLY, None)?;
    if b.navigate(probe, "https://example.com/").is_ok() {
        return Err(Outcome::Fail("a remote URL was accepted".into()));
    }
    if b.navigate(probe, "file:///etc/hostname").is_ok() {
        return Err(Outcome::Fail("a file URL was accepted".into()));
    }

    // The page tries to leave by itself. It reports its origin two seconds later, so a report is
    // only possible if the page is still on the app origin.
    let (log, handler) = report_sink();
    open_app(&mut b, NAV_PAGES, Some(handler))?;
    let expected = format!("report:{APP_ORIGIN}");
    match wait_for(&mut b, &log, Duration::from_secs(6), |m| {
        m.starts_with("report:")
    }) {
        Some(got) if got == expected => Ok((
            Outcome::Pass,
            "remote and file URLs refused by the backend; the page's own navigation stayed on the app origin".into(),
        )),
        Some(got) => Err(Outcome::Fail(format!(
            "the page reported {got:?}, expected {expected:?}"
        ))),
        None => Err(Outcome::Fail(
            "no origin report within 6 s: the page may have left the app origin".into(),
        )),
    }
}

fn theme_injection() -> Step<Found> {
    pending("the Linux backend has no theme injection yet (set_theme is a Windows backend method)")
}

fn snapshot() -> Step<Found> {
    let mut b = backend()?;
    let (log, handler) = report_sink();
    let id = window(&mut b, &[], Some(handler))?;
    b.load_html(id, LOADED_HTML).map_err(fail)?;
    if wait_for(&mut b, &log, WAIT, |m| m == "report:loaded").is_none() {
        return Err(Outcome::Fail("the page did not load in 60 s".into()));
    }
    let png = b.snapshot(id).map_err(fail)?;
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < SIGNATURE.len() || png[..SIGNATURE.len()] != SIGNATURE {
        return Err(Outcome::Fail(format!(
            "the snapshot is not a PNG ({} bytes)",
            png.len()
        )));
    }
    Ok((
        Outcome::Pass,
        format!("a PNG of {} KiB of the loaded page", png.len() / 1024),
    ))
}

fn drag_regions() -> Step<Found> {
    let mut b = backend()?;
    let id = window(&mut b, START_ONLY, None)?;
    match b.set_drag_regions(id, &[]) {
        Err(Error::Unsupported { feature }) => pending(format!(
            "the Linux backend does not implement {feature} yet; the trait default answers Unsupported"
        )),
        Ok(()) => pending("set_drag_regions is accepted, but no drag behaviour check is written yet"),
        Err(e) => Err(fail(e)),
    }
}

fn dpi() -> Step<Found> {
    pending("the backend has no public scale-factor query, so the check cannot compare it yet")
}
