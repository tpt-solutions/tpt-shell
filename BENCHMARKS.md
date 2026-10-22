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
| Blob, 4 MiB read by the page through the app's blob URL, 8 reads in a row, median per run, on the earlier asset-closure path (7 runs) | 36.3, 37.3, 38.3, 49.9, 54.5, 62.2, 65.9 MB/s | not the production path; see below |
| **Blob, 4 MiB stored with the facade's `put_blob` and read by the page through its URL, 8 reads in a row, median per run** | **Earlier: 102.1, 100.8, 99.6, 98.0, 98.7, 30.0, 63.3, 51.5, 68.3 MB/s. Logged later, 7 runs: 101.1, 83.9, 95.8, 105.9, 101.8, 93.8, 103.8 MB/s** | ≥ 50 MB/s, sustained: **met in the 7 logged runs (all ≥ 84 MB/s). Three earlier runs were lower, and their cause is not known** |

What the numbers mean:

- **An earlier IPC figure of 16.7 ms was the harness, not the channel.** The conformance pump waited with `Sleep(1)`, and on Windows that can take a full timer tick, about 16 ms. The ping timestamp is taken when the message is dispatched, so it inherited that wait. The pump now waits on the message queue, with a 1 ms timeout. The application's own loop (`GetMessageW`) never had this problem.
- **The blob figure now times only the transfer.** The earlier figure of 1.4 MB/s counted page start-up, about 2 to 4 s. The whole check, start-up included, still takes 2 to 4 s.
- **The blob budget is a sustained rate, measured as the median of eight transfers in a row.** Single transfers are one sample, so they are not what the budget asks about. The production path (the facade's `put_blob` store, read through its URL) ran at about 99 MB/s in five runs, then 30 MB/s in one, then 52 to 68 MB/s in three runs. Seven later runs were logged, and all were 84 to 106 MB/s. Those logged runs were checked against total CPU use and against the count of WebView2 processes left over from earlier checks. Neither tracked the result, so neither explains the three lower runs. Their cause is not known, and the budget is met in the logged runs. The earlier figures below were a different path, with a copy in the asset closure, and they are not comparable with this one.
- **The production path is measured through the facade's own lookup.** The harness calls `tpt_shell::__private::blob_asset_for_path`, which is the same lookup the app's asset source makes for `__tpt/blob/<id>`. In the runs above the response took a median 31 to 32 ms, and the lookup 0.85 ms of it.
- **The earlier path had a copy in the asset closure. That copy is not the cause.** Copying 4 MiB takes about 1.5 ms (PowerShell `Clone()`, 8 runs after warm-up), against transfers of about 100 ms. The page now times each read in two parts. The response (headers) takes most of each read, about 52 to 78 ms, and the body takes the rest, about 9 to 31 ms. So the time is spent before WebView2 hands the response to the page, not in reading the bytes. The host's own work is small. The asset closure takes a median 1.6 ms, measured on the host. The backend then copies the body again into a stream (`SHCreateMemStream` in `webview2.rs`), which is about 1.5 ms by the same estimate, not measured. That is about 3 ms of host copying in a response that takes about 55 ms. The rest is WebView2's own handling of the response between the host and the page.
- **Not measured yet:** the 64 MiB store limit under load, and the blob channel on macOS and Linux.

The conformance suite checks correctness, so every check above passes, including the two budget checks. The budgets are judged from the numbers here, and both blob budget numbers are not met.

Method: `TPT_WEBVIEW2_LOADER=<path to WebView2Loader.dll> cargo test --release --manifest-path conformance/Cargo.toml -- --nocapture`. The results go to `conformance/out/windows.json` (git-ignored).

## Supervisor idle memory (Windows)

| Measure | Result | Budget |
|---|---|---|
| Private memory, release `tpt-shell-supervisor` supervising a healthy child | 3.2 MB | 5 MB |
| Working set, same run | 7.0 MB | 5 MB |

Method: start `tpt-shell-supervisor -- supervisor-child beat` (release builds), wait 8 seconds, read the supervisor process's counters with `Get-Process`. The working set includes shared system DLL pages, so it is the larger number. The budget in §13 does not say which one it means.

## Hello example on Windows: cold start, idle memory, size (release, 2026-10-23)

Measured with the release `examples/hello`, a WebView2 loader from the Microsoft.Web.WebView2 package, and the Evergreen runtime 155.0.4283.45. Desktop in use during the runs.

| Measure | Result | Budget |
|---|---|---|
| Binary size, release `hello.exe` | 1,049,600 bytes | 4 MB stripped: met |
| Cold start: launch to the window appearing, 3 runs | 1,584, 1,690, 1,830 ms | to first paint ≤ 500 ms: **not met** |
| Shell process private memory, 12 s after the window appeared | 5.4 MB | ≤ 25 MB: met |
| Shell process working set, same run | 23.2 MB | ≤ 25 MB: met |
| WebView2 processes started by the shell (6), private memory, same run | 139.7 MB | not in the shell's budget (see below) |
| WebView2 processes, working set, same run | 299.8 MB | not in the shell's budget (see below) |

- **Cold start.** The window appears only after the WebView2 controller exists, because `create` waits for it. So the 1.6 to 1.8 s is mostly WebView2 set-up, which matches the 0.8 to 2.0 s controller creation noted under M1. First paint of the page itself is not timed separately, so this is an upper bound on the window and not a first-paint figure. The budget is not met.
- **Memory.** The spec does not say which memory measure its budget means, or whether the WebView2 runtime counts toward it. My recommendation, which you can change: the budget applies to the shell's own process, measured as private memory, and the runtime is reported separately. On that reading the shell is well inside the budget. Under working set it is also inside, at 23.2 MB.
- **Size.** The earlier figure of 706,048 bytes (2026-10-22) is out of date. The release build is now 1,049,600 bytes. Still within the 4 MB budget.

Method: `Start-Process` on the release example, polling the process for a non-zero main-window handle, every 5 ms. Memory is read with `Get-Process` after 12 seconds, with the WebView2 children found through their parent process IDs.

## Cold start, split (conformance probe, release, 2026-10-23)

The probe is `cold_start_probe` in `conformance/tests/conformance/windows.rs`, run with `--ignored`. It times on the host, from the start of the window's creation to the page's `DOMContentLoaded`, over seven runs.

| Step | Median | Range |
|---|---|---|
| Window created, controller included | 1,086 ms | 480 to 1,642 ms |
| Page parsed (`DOMContentLoaded`), from the start | 2,951 ms | 743 to 3,118 ms |

- The budget is first paint within 500 ms. Neither step is first paint, and the page is parsed well after the window exists, so the budget is not met on either measure.
- About a third of the time is the window and controller. The rest is WebView2 loading the document after the handover, which the probe does not split further.
- **Probe limits.** The probe stops at `DOMContentLoaded`, not at a painted frame. Its paint time is the screen measure in the section above. (The probe's window was hidden by the bug fixed above, so animation frames were never a fair test of it.) The probe navigates to the app origin, as the app does, and loads an external script. An earlier version used an inline script, which never ran. That is the app's content security policy at work: it blocks inline scripts on the app origin, as the `csp` check confirms. It was not a defect in navigation. Timings are the same on the navigate path as before: a median of about 3 s to `DOMContentLoaded` in the earlier runs.

## A bug found by looking at the screen: the shell's windows were blank (fixed, 2026-10-23)

Every window the shell showed on Windows drew nothing. The page loaded and ran, but its WebView2 content windows were hidden: `IsWindowVisible` was false for `Chrome_WidgetWin_0`, `Chrome_WidgetWin_1` and the D3D window, even though they had full size. The shell never set the controller visible. WebView2 applies that change while messages are pumped, so the content windows became visible only after the pump ran.

The conformance checks could not see this, because they read page titles. The fix binds `put_IsVisible` (slot 4 of `ICoreWebView2Controller`) and sets it when a controller is created and again when a window is shown. A test, `the_webview_is_visible_once_its_window_is_shown`, waits up to 10 seconds for a visible content window. It fails without the fix and passes with it.

The numbers above for the page channel and the cold-start probe were taken before the fix. The conformance suite was run again afterwards, and the results are below.

| Check after the fix (release, 2026-10-23) | Result |
|---|---|
| IPC round trip, 200 pings, p95 | 0.20 ms (budget 5 ms: met) |
| Blob, 4 MiB stored and read through its URL, median of 8 | 101.2 MB/s (budget 50 MB/s: met in this run) |
| All other conformance checks | pass, or pending as before |

## First paint on screen (release hello, 2026-10-23, after the fix)

Timed from the screen: the release `hello` example was launched, and a pixel at the centre of its window was polled until it turned dark, which is the page's background. Five launches:

| Run | Window appears | Page painted on screen |
|---|---|---|
| 1 | 2,899 ms | 2,999 ms |
| 2 | 1,917 ms | 1,935 ms |
| 3 | 1,893 ms | 1,905 ms |
| 4 | 1,501 ms | 1,524 ms |
| 5 | 1,228 ms | 1,252 ms |

The page paints about 20 ms after its window appears, so the time is in window and controller set-up. The budget (first paint within 500 ms) is **not met**: the first paint is 1.25 to 3.0 s.

## Not measured yet

- The spec's reading of the shell memory budget, which needs a decision (see above).
- Whether the cold-start budget can be met at all: the window waits for the WebView2 controller, and that is most of the time.
- macOS and Linux builds.

The benchmark run is scripted in `tools/run-benchmarks.sh`. Use it so every result follows the same method.
