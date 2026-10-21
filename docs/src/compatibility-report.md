# Compatibility report

> **Template. Not published.** This page is a template for the published compatibility report
> (todo M4, M5 and M11). It is filled in by the publish step, from the conformance run on each
> platform. Until then, the status of each platform is in [Platform notes](platform-notes.md).

## What the report covers

The report lists each conformance check, and the result on each platform. The checks come from the
conformance suite in `conformance/`:

- IPC round trip
- large blobs
- streams and backpressure
- custom protocol
- content security policy
- navigation policy
- theme injection
- snapshot
- drag regions
- DPI

A check that needs a backend which is not built reports as pending. Pending is not a pass.

## Run details

| Field | Value |
|---|---|
| tpt-shell version or commit | _to fill_ |
| Date of run | _to fill_ |
| Windows build and WebView2 runtime version | _to fill_ |
| macOS version and WKWebView | _to fill_ |
| Linux distribution and WebKitGTK version | _to fill_ |
| Hardware used | _to fill_ (CI runners only, per decision O6) |
| Rust toolchain | _to fill_ (MSRV 1.90) |

## Results

| Check | Windows | macOS | Linux |
|---|---|---|---|
| IPC round trip | _to fill_ | _to fill_ | _to fill_ |
| Large blobs | _to fill_ | _to fill_ | _to fill_ |
| Streams and backpressure | _to fill_ | _to fill_ | _to fill_ |
| Custom protocol | _to fill_ | _to fill_ | _to fill_ |
| Content security policy | _to fill_ | _to fill_ | _to fill_ |
| Navigation policy | _to fill_ | _to fill_ | _to fill_ |
| Theme injection | _to fill_ | _to fill_ | _to fill_ |
| Snapshot | _to fill_ | _to fill_ | _to fill_ |
| Drag regions | _to fill_ | _to fill_ | _to fill_ |
| DPI | _to fill_ | _to fill_ | _to fill_ |

Each cell is one of: pass, fail, pending (backend not built), or not run. Explain every fail and every
pending in a note below the table.

## Known gaps

_To fill from the run._ List each gap with the todo item it belongs to.

## How to read this report

- A pass on Windows does not show a pass on macOS or Linux.
- A cross-compiled build is not a run. Only a row marked as run on that platform counts.
- The report does not measure performance. The benchmarks are in `BENCHMARKS.md`.
