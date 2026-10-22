// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Measures what the router itself costs per call: parse, dispatch and encode, in this process.
//! It does not measure the page channel (WebView2 `postMessage`), which the Windows conformance
//! suite measures. The budget in spec §13 (p95 ≤ 5 ms) is for the whole round trip, so this
//! number is only the router's share of it.
//!
//! Ignored by default, so an ordinary test run is not slowed down. Run it with:
//!   cargo test -p tpt-shell-ipc --release --test router_latency -- --ignored --nocapture

use std::time::Instant;

use tpt_shell_ipc::{Limits, Router, Value};

const CALLS: usize = 20_000;

#[test]
#[ignore = "a measurement, run on request (see the module notes)"]
fn the_router_costs_little_per_call() {
    let mut router = Router::new();
    router.register("echo", |args: &Value| Ok(args.clone()));
    let limits = Limits::default();

    let mut micros = Vec::with_capacity(CALLS);
    for id in 0..CALLS {
        let request =
            format!(r#"{{"v":1,"id":{id},"cmd":"echo","args":{{"text":"hello","n":3}}}}"#);
        let start = Instant::now();
        let response = router.handle(request.as_bytes(), &limits);
        micros.push(start.elapsed().as_secs_f64() * 1e6);
        assert!(response.contains("\"ok\":true"), "{response}");
    }
    micros.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let at = |p: f64| micros[((micros.len() as f64 - 1.0) * p).round() as usize];
    println!(
        "router only, {CALLS} calls: p50 {:.1} us, p95 {:.1} us, p99 {:.1} us, max {:.1} us",
        at(0.50),
        at(0.95),
        at(0.99),
        micros[micros.len() - 1]
    );
}
