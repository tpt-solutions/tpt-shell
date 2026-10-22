# Benchmarks

Measured numbers for tpt-shell, with how each was taken. A budget is met only when the
measurement below says so. Budgets are from spec §13.

## Binary size (hello-world, Windows)

| Item | Result | Budget |
|---|---|---|
| `examples/hello.exe`, release, MSVC | 706,048 bytes (690 KiB) | 4 MB stripped |

The earlier figure, 398,336 bytes on 2026-10-21, predates the audit log and the blob store, which the facade now links. The growth is the audit crate's SHA-256 and Ed25519 code and the blob code. The hello example also gained single-instance support, which is small.

Method:

- Build: `cargo build --release -p tpt-shell --example hello` on Windows 11, rustc 1.97.1,
  workspace at commit e4d1577 plus the changes made since, including the single-instance
  support in the hello example.
- The release profile on Windows keeps debug symbols in a separate `.pdb`, so the `.exe`
  has none. The result is the same with `CARGO_PROFILE_RELEASE_STRIP=symbols`.
- The example opens one window with one embedded page. It loads `WebView2Loader.dll` at run
  time, so the runtime is not part of the file. The Evergreen WebView2 runtime is installed
  separately.
- Measured on 2026-10-22. Rebuild and re-measure after a dependency change.

## Router cost per IPC call (in process, not the round trip)

| Measure | Result | Budget |
|---|---|---|
| Router only (parse, dispatch, encode), 20,000 calls: p50 / p95 / p99 | 1.4 / 1.5 / 2.3 µs | the whole round trip, p95 ≤ 5 ms |

This is the router's share of an IPC call, and nothing else. It does not include the page channel (WebView2 `postMessage` in both directions), which is most of the round trip. The IPC budget stays unmet until the channel is measured: the Windows conformance suite does that, and it needs `WebView2Loader.dll`.

Method: `cargo test -p tpt-shell-ipc --release --test router_latency -- --ignored --nocapture`. A no-op echo command, default limits, one request per call. The test is in `crates/tpt-shell-ipc/tests/router_latency.rs`.

## Supervisor idle memory (Windows)

| Measure | Result | Budget |
|---|---|---|
| Private memory, release `tpt-shell-supervisor` supervising a healthy child | 3.2 MB | 5 MB |
| Working set, same run | 7.0 MB | 5 MB |

Method: start `tpt-shell-supervisor -- supervisor-child beat` (release builds), wait 8 seconds, read the supervisor process's counters with `Get-Process`. The working set includes shared system DLL pages, so it is the larger number. The budget in §13 does not say which one it means.

## Not measured yet

- Cold start to first paint (spec §13 budget 500 ms). Partial numbers are in `todo.md`
  under M1 (controller creation 0.8–2.0 s, total 2.1–2.3 s, so the budget is not met).
- IPC round trip through the page channel (budget p95 ≤ 5 ms). Only the router's share is measured, above.
- Blob channel throughput (budget ≥ 50 MB/s).
- Idle shell memory (budget ≤ 25 MB) and supervisor idle memory (budget ≤ 5 MB).
- macOS and Linux builds.

The benchmark run is scripted in `tools/run-benchmarks.sh`. Use it so every result follows the same method.
