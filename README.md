# tpt-shell

A webview app shell for Rust that is licence-clean, secure by default, and built for kiosks, regulated work and AI agents.

Owner: TPT Solutions

> **Status: pre-alpha.** The Windows backend (WebView2) is the one verified to open windows, serve pages and run the typed, capability-checked command channel on a desktop. The macOS and Linux backends are written but partial, and have not yet passed the full conformance suite. The API is still changing. See [platform-notes.md](docs/src/platform-notes.md) for what each platform does, [todo.md](todo.md) for the project checklist and [spec.txt](spec.txt) for the full design.

## What it is

tpt-shell lets a Rust program open native windows that host web UIs (HTML/CSS/JS) in the operating system's own webview, and talk to the Rust backend over a typed, validated, permissioned, audited channel.

It does not use wry, tao, winit, Tauri or any other windowing or webview crate. It talks to the platform directly:

- Windows: Win32 + WebView2 (COM)
- macOS: AppKit + WKWebView (Objective-C runtime)
- Linux: GTK + WebKitGTK (dlopen)

## Principles

- **Secure by default.** Nothing privileged works until the manifest grants it.
- **Licence-clean.** Dependencies must be MIT, or dual-licensed with MIT. Apache-2.0-only crates are rejected. Enforced by `cargo-deny` (see [deny.toml](deny.toml)).
- **No telemetry.** The shell does not collect or send usage data.
- **No network by default.** The shell opens no sockets unless a feature is enabled.
- **Confined unsafe code.** The `unsafe` code and FFI are in the platform, device and kiosk crates, each in its own modules.

## Features

- **Native windows.** Windows that host HTML/CSS/JS in the system webview. On Windows, the verified backend, you get per-monitor placement, named or ephemeral profiles, a menu bar, hidden windows and a choice of WebView2 channel.
- **Typed command channel.** Page calls are validated and checked against the manifest's permissions, and every call is written to a hash-chained audit log (`tpt-shell audit verify | export | view`).
- **Devices.** Serial, HID and USB transports behind a scoped hub, with virtual devices for tests and ESC/POS and ZPL receipt and label builders. HID and WinUSB opening is not yet verified on hardware.
- **Kiosk and supervision.** A signed kiosk config, state snapshots, an idle reset, and a supervisor that restarts the app with backoff and a heartbeat.
- **Agents.** An MCP-style session over stdio, or localhost HTTP with the `mcp-http` feature, with a snapshot of the accessible tree and audited actions.
- **Record and replay.** Recordings of requests and page input that replay through the app's own router.
- **Testing.** A test driver, a visual comparer, accessibility rules and artifact collection (`tpt-shell test`).
- **Compliance.** CycloneDX and SPDX SBOMs, licence notices and attestation output.

The workspace is split into focused crates under `crates/`. The `tpt-shell` crate is the public facade. Each crate's `README.md` and `CHANGELOG.md` describe it in detail.

## Quickstart

On Windows, the hello example opens one window that shows one embedded page. It takes the path to `WebView2Loader.dll` as its argument. The repository vendors the x64 copy in `redist/win-x64/`:

```sh
cargo run -p tpt-shell --example hello -- redist/win-x64/WebView2Loader.dll
```

The example is a minimal app. Its core is this:

```rust
use tpt_shell::{App, Window};

const PAGE: &str = "<!doctype html><title>hello</title><h1>hello from tpt-shell</h1>";

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("hello")
        .asset("index.html", "text/html", PAGE)
        .window(Window::new("main").title("hello"))
        .run()
}
```

`App::loader(path)` sets the loader's path. It is optional: without it, the app looks for `WebView2Loader.dll` beside the program.

The `tpt-shell` CLI makes and runs projects. Install it from a checkout, then create and run an app:

```sh
cargo install --path crates/tpt-shell-cli
tpt-shell new rust my-app
tpt-shell run my-app
```

`tpt-shell run` copies the vendored `WebView2Loader.dll` beside the built program, so no manual copy is needed. The [quickstart](docs/src/quickstart.md) covers the steps in full, including `tpt-shell dev` for previewing a project's pages in a browser.

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo deny check
```

Requires Rust 1.90 or later. `cargo deny` needs the `cargo-deny` tool installed. No Node.js is needed to build.

## Licence

Dual-licensed under either of

- [MIT licence](LICENSE-MIT)
- [Apache licence, version 2.0](LICENSE-APACHE)

at your option.

Third-party components are listed in [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES). That file includes the WebView2 loader (`WebView2Loader.dll`, BSD-3-Clause, from the Microsoft.Web.WebView2 NuGet package) and the decision to use it (O1).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work, as defined in the Apache-2.0 licence, shall be dual licensed as above, without any additional terms or conditions.

## Security

Report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](CONTRIBUTING.md).
