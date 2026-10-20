# tpt-shell

A webview app shell for Rust that is licence-clean, secure by default, and built for kiosks, regulated work and AI agents.

Owner: TPT Solutions

> **Status: pre-alpha.** Milestone M0 (repo, policy, CI skeleton) is in progress. There is no usable API yet. See [todo.md](todo.md) for the project checklist and [spec.txt](spec.txt) for the full design.

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

See [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
