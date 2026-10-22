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

This is the router's share of an IPC call, and nothing else. It does not include the page channel (WebView2 `postMessage` in both directions), which is most of the round trip. The page channel is measured below.

Method: `cargo test -p tpt-shell-ipc --release --test router_latency -- --ignored --nocapture`. A no-op echo command, default limits, one request per call. The test is in `crates/tpt-shell-ipc/tests/router_latency.rs`.

## Page channel on Windows (WebView2, release)

Measured 2026-10-23 with the Windows conformance suite, on Windows 11 with the Evergreen WebView2 runtime 155.0.4283.45 and `WebView2Loader.dll` from the Microsoft.Web.WebView2 package (signed by Microsoft Corporation). Release build.

| Measure | Result | Budget |
|---|---|---|
| IPC round trip, page to host and back, 200 round trips: p95 gap between pings, 3 runs | 0.12, 0.34, 0.44, 0.70, 0.81 ms (5 runs) | p95 ≤ 5 ms: **met** |
| Blob, 4 MiB host to page as a message, 8 transfers in a row, median per run (5 runs, the first two before the copy and split changes) | 38.9, 46.2, 47.9, 52.5, 44.1 MB/s | ≥ 50 MB/s, sustained: **around the budget, not reliably met** |
| Blob, 4 MiB read by the page through the app's blob URL, 8 reads in a row, median per run (7 runs, the first two before the response split) | 36.3, 37.3, 38.3, 49.9, 54.5, 62.2, 65.9 MB/s | ≥ 50 MB/s, sustained: **around the budget, not reliably met** |

What the numbers mean:

- **An earlier IPC figure of 16.7 ms was the harness, not the channel.** The conformance pump waited with `Sleep(1)`, and on Windows that can take a full timer tick, about 16 ms. The ping timestamp is taken when the message is dispatched, so it inherited that wait. The pump now waits on the message queue, with a 1 ms timeout. The application's own loop (`GetMessageW`) never had this problem.
- **The blob figure now times only the transfer.** The earlier figure of 1.4 MB/s counted page start-up, about 2 to 4 s. The whole check, start-up included, still takes 2 to 4 s.
- **The blob budget is a sustained rate, and the sustained measure is now the median of eight transfers in a row.** Single transfers looked faster (39 to 86 MB/s over four runs), but one transfer is one sample, so it is not what the budget asks about. The median of eight is what the budget asks about. Across runs it sits around 50 MB/s, above it in some runs and below in others, so the budget is not reliably met. The runs were on a desktop in use, which adds variation the measurement cannot separate out.
- **The fetch path includes the asset closure's copy of the 4 MiB body, and that copy is not the cause.** Copying 4 MiB takes about 1.5 ms (PowerShell `Clone()`, 8 runs after warm-up), against transfers of about 100 ms. The page now times each read in two parts. The response (headers) takes most of each read, about 52 to 78 ms, and the body takes the rest, about 9 to 31 ms. So the time is spent before WebView2 hands the response to the page, not in reading the bytes. That is in the host's request handling or in WebView2's transfer of the response. It is not separated further yet.
- **Not measured yet:** the facade's `put_blob` store and the 64 MiB limit, and the blob channel on macOS and Linux.

The conformance suite checks correctness, so every check above passes, including the two budget checks. The budgets are judged from the numbers here, and both blob budget numbers are not met.

Method: `TPT_WEBVIEW2_LOADER=<path to WebView2Loader.dll> cargo test --release --manifest-path conformance/Cargo.toml -- --nocapture`. The results go to `conformance/out/windows.json` (git-ignored).

## Supervisor idle memory (Windows)

| Measure | Result | Budget |
|---|---|---|
| Private memory, release `tpt-shell-supervisor` supervising a healthy child | 3.2 MB | 5 MB |
| Working set, same run | 7.0 MB | 5 MB |

Method: start `tpt-shell-supervisor -- supervisor-child beat` (release builds), wait 8 seconds, read the supervisor process's counters with `Get-Process`. The working set includes shared system DLL pages, so it is the larger number. The budget in §13 does not say which one it means.

## Not measured yet

- Cold start to first paint (spec §13 budget 500 ms). Partial numbers are in `todo.md`
  under M1 (controller creation 0.8–2.0 s, total 2.1–2.3 s, so the budget is not met).
- Idle shell memory (budget ≤ 25 MB) and supervisor idle memory (budget ≤ 5 MB).
- macOS and Linux builds.

The benchmark run is scripted in `tools/run-benchmarks.sh`. Use it so every result follows the same method.
