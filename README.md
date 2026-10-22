# tpt-shell

A webview app shell for Rust that is licence-clean, secure by default, and built for kiosks, regulated work and AI agents.

Owner: TPT Solutions

> **Status: pre-alpha.** The Windows backend (WebView2) is the most complete: it opens windows, serves pages, and runs the typed, capability-checked command channel. The macOS and Linux backends are partial and have not yet passed the full conformance suite. The API is still changing. See [platform-notes.md](docs/src/platform-notes.md) for what each platform does, [todo.md](todo.md) for the project checklist and [spec.txt](spec.txt) for the full design.

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
- **Small unsafe surface.** All FFI lives in isolated platform modules.

## Quickstart

On Windows, the hello example opens one window that shows one embedded page. It needs the path to `WebView2Loader.dll`:

```sh
cargo run -p tpt-shell --example hello -- <path to WebView2Loader.dll>
```

The example is the smallest app. Its core is this:

```rust
use tpt_shell::{App, Window};

const PAGE: &str = "<!doctype html><title>hello</title><h1>hello from tpt-shell</h1>";

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("hello")
        .loader("WebView2Loader.dll")
        .asset("index.html", "text/html", PAGE)
        .window(Window::new("main").title("hello"))
        .run()
}
```

## Build

```sh
cargo build --workspace
cargo test --workspace
cargo deny check
```

Requires a stable Rust toolchain. No Node.js is needed.

## Licence

Dual-licensed under either of

- [MIT licence](LICENSE-MIT)
- [Apache licence, version 2.0](LICENSE-APACHE)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this work, as defined in the Apache-2.0 licence, shall be dual licensed as above, without any additional terms or conditions.

## Security

Report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](CONTRIBUTING.md).
