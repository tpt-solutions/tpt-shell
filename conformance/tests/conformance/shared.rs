// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The checks shared by the GTK and macOS backends. Each check is generic over [`Host`], so the same
//! pages and steps run on every backend that implements it. The platform files (`linux.rs`,
//! `macos.rs`) provide the backend's constructor and its main-loop pump, and run the table.
//!
//! The backend has no page-title query, so the pages in `pages/linux/` report their results through
//! the message channel (`report:...`). A check that cannot run on a backend is pending, with the reason.

use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tpt_shell_conformance::stream::{self, StreamHost};
use tpt_shell_conformance::{CheckResult, Outcome, RunReport, CHECKS};
use tpt_shell_core::backend::{Asset, MessageHandler, WebviewBackend, WindowBackend, WindowSpec};
use tpt_shell_core::platform::ThemeBackend;
use tpt_shell_core::theme::Theme;
use tpt_shell_core::{ColorScheme, LogicalRect, LogicalSize, Result as CoreResult};

/// What a platform backend must provide for the shared checks.
pub trait Host: WebviewBackend + WindowBackend + ThemeBackend + Sized {
    /// The origin the backend serves app pages from.
    const ORIGIN: &'static str;

    /// A fresh backend, or the outcome to report instead (skipped, or failed under strict mode).
    fn open() -> Step<Self>;

    /// Runs the main loop until `done` holds or `timeout` passes. Returns whether `done` held.
    fn pump(&mut self, done: &mut dyn FnMut() -> bool, timeout: Duration) -> bool;

    /// Sets the theme for windows created after this call.
    fn set_theme(&mut self, theme: Theme) -> CoreResult<()>;
}

/// The URL of the app's start page on this backend.
fn start_url<H: Host>() -> String {
    format!("{}/index.html", H::ORIGIN)
}

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
pub type Step<T> = Result<T, Outcome>;
pub type Found = (Outcome, String);
type CheckFn = fn() -> Step<Found>;

const START: &str = include_str!("../../pages/start.html");
const IPC_JS: &str = include_str!("../../pages/ipc.js");
const BLOB_JS: &str = include_str!("../../pages/linux/blob.js");
const PROTOCOL_JS: &str = include_str!("../../pages/linux/protocol.js");
const CSP_HTML: &str = include_str!("../../pages/linux/csp.html");
const CSP_JS: &str = include_str!("../../pages/linux/csp.js");
const NAV_JS: &str = include_str!("../../pages/linux/nav.js");
const FETCH_JS: &str = include_str!("../../pages/fetch.js");

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

/// The page for the DPI check: it posts its device pixel ratio.
const DPR_HTML: &str = r#"<!doctype html><body><script>window.chrome.webview.postMessage("report:ratio:" + window.devicePixelRatio);</script></body>"#;
/// The streams check's pages: the shared stream page with its own start page.
const STREAM_PAGES: Pages = &[
    (
        "index.html",
        "text/html",
        "<!doctype html><title>waiting</title><script src=\"page.js\"></script>",
    ),
    ("page.js", "text/javascript", stream::PAGE_JS),
];

/// The page for the theme check: on load it reports the colour scheme and the accent token.
const THEME_HTML: &str = r#"<!doctype html><script>document.addEventListener('DOMContentLoaded', function () { window.chrome.webview.postMessage('report:theme:' + document.documentElement.dataset.colorScheme + '|' + document.documentElement.style.getPropertyValue('--tpt-accent').trim()); });</script>"#;

/// The page for the drag check: it reports when it has loaded.
const DRAG_HTML: &str = r#"<!doctype html><body><script>window.chrome.webview.postMessage("report:ready");</script></body>"#;

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

/// Creates a window whose app origin serves `pages` and shows it. The page is not loaded yet.
fn window<H: Host>(b: &mut H, pages: Pages, handler: Option<MessageHandler>) -> Step<H::Id> {
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
fn open_app<H: Host>(b: &mut H, pages: Pages, handler: Option<MessageHandler>) -> Step<H::Id> {
    let id = window(b, pages, handler)?;
    b.navigate(id, &start_url::<H>()).map_err(fail)?;
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
fn wait_for<H: Host>(
    b: &mut H,
    log: &Reports,
    timeout: Duration,
    found: impl Fn(&str) -> bool,
) -> Option<String> {
    let _ = b.pump(&mut || log.borrow().iter().any(|m| found(m)), timeout);
    log.borrow().iter().find(|m| found(m)).cloned()
}

fn ipc_round_trip<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
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
    b.pump(&mut || stamps.borrow().len() >= ROUNDS, WAIT);
    let _ = b.pump(&mut || false, QUIET);
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
fn large_blobs<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let blob = format!("{}z", "a".repeat(BLOB_BYTES - 1));
    // When the page asked for each blob. The page reports after the last reply.
    let asks: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
    let log: Reports = Rc::new(RefCell::new(Vec::new()));
    let (asks_seen, log_seen) = (asks.clone(), log.clone());
    let handler: MessageHandler = Box::new(move |message: &str| {
        if message == "blob" {
            asks_seen.borrow_mut().push(Instant::now());
            Some(blob.clone())
        } else {
            log_seen.borrow_mut().push(message.to_string());
            None
        }
    });
    open_app(&mut b, BLOB_PAGES, Some(handler))?;
    let expected = format!("report:blob:{BLOB_BYTES}:z:{BLOB_COUNT}");
    let report = wait_for(&mut b, &log, WAIT, |m| m.starts_with("report:blob:"));
    let done = Instant::now();
    match report {
        Some(got) if got == expected => {}
        Some(got) => {
            return Err(Outcome::Fail(format!(
                "the page reported {got:?}, expected {expected:?}"
            )))
        }
        None => return Err(Outcome::Fail("the page did not finish in 60 s".into())),
    }
    let asks = asks.borrow();
    if asks.len() != BLOB_COUNT {
        return Err(Outcome::Fail(format!(
            "the page asked {} times, expected {BLOB_COUNT}",
            asks.len()
        )));
    }
    // Each transfer runs from one request to the next; the last one ends when the page reports.
    let ends: Vec<Instant> = asks.iter().skip(1).copied().chain([done]).collect();
    let mut rates: Vec<f64> = asks
        .iter()
        .zip(&ends)
        .map(|(from, to)| {
            (BLOB_BYTES as f64 / 1_000_000.0) / to.duration_since(*from).as_secs_f64()
        })
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let message_detail = format!(
        "{BLOB_COUNT} transfers of {} MiB as messages, median {:.1} MB/s, slowest {:.1} MB/s (budget 50 MB/s sustained, spec 13)",
        BLOB_BYTES / (1024 * 1024),
        rates[rates.len() / 2],
        rates[0]
    );
    drop(asks);
    let (fetch_outcome, fetch_detail) = large_blobs_fetch::<H>()?;
    Ok((fetch_outcome, format!("{message_detail}; {fetch_detail}")))
}

/// The same size of blob read eight times in a row, through the app origin's asset path. The
/// Linux backend has no blob store yet (the facade's `put_blob` is Windows only), so the blob is an
/// asset the page fetches. That exercises the same request path, but not the facade's lookup.
fn large_blobs_fetch<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let mut bytes = vec![b'a'; BLOB_BYTES];
    bytes[BLOB_BYTES - 1] = b'z';
    let body = Rc::new(bytes);
    let page_js = format!(
        "var BLOB_PATH = \"blob\";
{FETCH_JS}"
    );
    let (log, handler) = report_sink();
    let id = window(&mut b, &[], Some(handler))?;
    b.set_asset_source(
        id,
        Box::new(move |path| match path {
            "index.html" => Some(Asset {
                content_type: "text/html".into(),
                body: START.as_bytes().to_vec(),
            }),
            "page.js" => Some(Asset {
                content_type: "text/javascript".into(),
                body: page_js.clone().into_bytes(),
            }),
            "blob" => Some(Asset {
                content_type: "application/octet-stream".into(),
                body: body.as_ref().clone(),
            }),
            _ => None,
        }),
    )
    .map_err(fail)?;
    b.navigate(id, &start_url::<H>()).map_err(fail)?;
    let report = wait_for(&mut b, &log, WAIT, |m| m.starts_with("report:fetch:"))
        .ok_or_else(|| Outcome::Fail("the page did not finish the blob reads in 60 s".into()))?;
    // The page reports `fetch:<length>:<last>:<count>:<median total ms>:<median response ms>`.
    let expected_prefix = format!("report:fetch:{BLOB_BYTES}:z:{BLOB_COUNT}:");
    let times: Option<(f64, f64)> = report
        .strip_prefix(&expected_prefix)
        .and_then(|rest| rest.split_once(':'))
        .and_then(|(total, headers)| Some((total.parse().ok()?, headers.parse().ok()?)));
    let Some((total_ms, headers_ms)) = times else {
        return Ok((
            Outcome::Fail(format!(
                "the page reported {report:?}, expected {expected_prefix:?} and two times"
            )),
            String::new(),
        ));
    };
    let mb_per_s = (BLOB_BYTES as f64 / 1_000_000.0) / (total_ms / 1000.0);
    Ok((
        Outcome::Pass,
        format!(
            "{BLOB_COUNT} reads of {} MiB through the app origin's asset path, median {mb_per_s:.1} MB/s; median response {headers_ms:.1} ms of {total_ms:.1} ms (facade blob store not used on Linux)",
            BLOB_BYTES / (1024 * 1024)
        ),
    ))
}

fn streams_backpressure<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let host = Rc::new(RefCell::new(StreamHost::new()));
    let seen = host.clone();
    let handler: MessageHandler = Box::new(move |message: &str| {
        seen.borrow_mut().on_message(message);
        None
    });
    let id = window(&mut b, STREAM_PAGES, Some(handler))?;
    b.navigate(id, &start_url::<H>()).map_err(fail)?;
    let deadline = Instant::now() + WAIT;
    loop {
        let posts = host.borrow_mut().next_posts();
        for message in posts {
            b.post_message(id, &message).map_err(fail)?;
        }
        if host.borrow().complete() || Instant::now() >= deadline {
            break;
        }
        b.pump(&mut || host.borrow().complete(), Duration::from_millis(5));
    }
    let verdict = stream::verdict(&host.borrow());
    verdict
        .map(|detail| (Outcome::Pass, detail))
        .map_err(Outcome::Fail)
}

fn custom_protocol<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let (log, handler) = report_sink();
    open_app(&mut b, PROTOCOL_PAGES, Some(handler))?;
    let expected = format!("report:{}|text/html|404", H::ORIGIN);
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

fn csp<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
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

fn navigation_policy<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
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
    let expected = format!("report:{}", H::ORIGIN);
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

fn theme_injection<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    b.set_theme(Theme {
        accent: "#123456".into(),
        ..Theme::default()
    })
    .map_err(fail)?;
    // The scheme the backend injects: light when the backend cannot read the system setting.
    let scheme = match ThemeBackend::color_scheme(&b) {
        Ok(ColorScheme::Dark) => "dark",
        _ => "light",
    };
    let (log, handler) = report_sink();
    let id = window(&mut b, START_ONLY, Some(handler))?;
    b.load_html(id, THEME_HTML).map_err(fail)?;
    let expected = format!("report:theme:{scheme}|#123456");
    match wait_for(&mut b, &log, WAIT, |m| m.starts_with("report:theme:")) {
        Some(got) if got == expected => Ok((
            Outcome::Pass,
            format!("accent token and colour scheme ({scheme}) reached a new page"),
        )),
        Some(got) => Err(Outcome::Fail(format!(
            "the page reported {got:?}, expected {expected:?}"
        ))),
        None => Err(Outcome::Fail(
            "the page did not report its theme in 60 s".into(),
        )),
    }
}

fn snapshot<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
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

fn drag_regions<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let (log, handler) = report_sink();
    let id = window(&mut b, START_ONLY, Some(handler))?;
    b.load_html(id, DRAG_HTML).map_err(fail)?;
    wait_for(&mut b, &log, WAIT, |m| m == "report:ready")
        .ok_or_else(|| Outcome::Fail("the page did not load in 60 s".into()))?;

    let region = LogicalRect {
        x: 10.0,
        y: 20.0,
        width: 200.0,
        height: 40.0,
    };
    b.set_drag_regions(id, &[region]).map_err(fail)?;
    let boxes = drag_boxes(&mut b, id, &log)?;
    let expected = [10.0, 20.0, 200.0, 40.0];
    if boxes.len() != 1
        || boxes[0]
            .iter()
            .zip(expected)
            .any(|(a, b)| (a - b).abs() > 0.5)
    {
        return Err(Outcome::Fail(format!(
            "the drag boxes are {boxes:?}, expected one box at {expected:?}"
        )));
    }

    // A primary press on the box must be taken by the host as a move request. If it reached the
    // page's handler, it would be logged before the marker that follows it.
    log.borrow_mut().clear();
    b.eval(
        id,
        "document.querySelector('[data-tpt-drag]').dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true, screenX: 50, screenY: 60 })); window.chrome.webview.postMessage('report:after-press');",
    )
    .map_err(fail)?;
    wait_for(&mut b, &log, WAIT, |m| m == "report:after-press")
        .ok_or_else(|| Outcome::Fail("the page did not answer after the press in 60 s".into()))?;
    if log
        .borrow()
        .iter()
        .any(|m| m.starts_with(tpt_shell_core::drag::MESSAGE_PREFIX))
    {
        return Err(Outcome::Fail(
            "the press reached the page's handler; the host did not take it as a move request"
                .into(),
        ));
    }

    // Setting no regions removes the boxes again.
    b.set_drag_regions(id, &[]).map_err(fail)?;
    if !drag_boxes(&mut b, id, &log)?.is_empty() {
        return Err(Outcome::Fail(
            "drag boxes remained after the regions were cleared".into(),
        ));
    }
    Ok((
        Outcome::Pass,
        "one drag box at the region's rectangle; a primary press on it was taken by the host as a window-move request and not passed to the page. The move itself is not measured (no window manager under Xvfb)".into(),
    ))
}

/// The drag boxes in the page, each as [left, top, width, height]. Waits for the page's answer.
fn drag_boxes<H: Host>(b: &mut H, id: H::Id, log: &Reports) -> Step<Vec<[f64; 4]>> {
    log.borrow_mut().clear();
    b.eval(
        id,
        "window.chrome.webview.postMessage('report:drag:' + JSON.stringify(Array.from(document.querySelectorAll('[data-tpt-drag]')).map(function (e) { var r = e.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; })));",
    )
    .map_err(fail)?;
    let answer = wait_for(b, log, WAIT, |m| m.starts_with("report:drag:"))
        .ok_or_else(|| Outcome::Fail("the page did not list its drag boxes in 60 s".into()))?;
    serde_json::from_str(&answer["report:drag:".len()..])
        .map_err(|e| Outcome::Fail(format!("the drag box list was not JSON: {e}")))
}

fn dpi<H: Host>() -> Step<Found> {
    let mut b = H::open()?;
    let (log, handler) = report_sink();
    let id = window(&mut b, START_ONLY, Some(handler))?;
    b.load_html(id, DPR_HTML).map_err(fail)?;
    let report =
        wait_for(&mut b, &log, WAIT, |m| m.starts_with("report:ratio:")).ok_or_else(|| {
            Outcome::Fail("the page did not report its device pixel ratio in 60 s".into())
        })?;
    let ratio: f64 = report["report:ratio:".len()..]
        .parse()
        .map_err(|_| Outcome::Fail(format!("the page reported {report:?}")))?;
    let scale = b.scale_factor(id).map_err(fail)?.get();
    if (scale - ratio).abs() > 0.01 {
        return Err(Outcome::Fail(format!(
            "the window reports scale {scale:.3} but the page reports devicePixelRatio {ratio:.3}"
        )));
    }
    Ok((
        Outcome::Pass,
        format!("the window's scale factor {scale:.3} matches the page's devicePixelRatio"),
    ))
}

/// Runs every check on the backend `H` and returns the report for `platform`. `backend` names the
/// toolkit in the report.
pub fn run_checks<H: Host>(platform: &str, backend: &str) -> RunReport {
    let table: [(&str, CheckFn); 10] = [
        ("ipc-round-trip", ipc_round_trip::<H>),
        ("large-blobs", large_blobs::<H>),
        ("streams-backpressure", streams_backpressure::<H>),
        ("custom-protocol", custom_protocol::<H>),
        ("csp", csp::<H>),
        ("navigation-policy", navigation_policy::<H>),
        ("theme-injection", theme_injection::<H>),
        ("snapshot", snapshot::<H>),
        ("drag-regions", drag_regions::<H>),
        ("dpi", dpi::<H>),
    ];
    let results = CHECKS
        .iter()
        .map(|name| {
            let Some((_, check)) = table.iter().find(|(n, _)| n == name) else {
                return result(
                    name,
                    Outcome::Fail("no check is written for this name".into()),
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
        platform: platform.into(),
        backend: backend.into(),
        results,
    }
}
