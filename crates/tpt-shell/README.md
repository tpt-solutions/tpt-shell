# tpt-shell

A webview app shell for Rust. It opens native windows that host HTML, CSS and JavaScript UIs in the operating system's own webview, and lets those pages call the Rust backend over a typed, validated, permissioned and audited channel.

This is the facade crate. Most apps depend on this crate alone and pull the others in through it.

> **Status: pre-alpha.** Windows (Win32 and WebView2) is the one backend verified to open windows and run the command channel on a desktop. The macOS and Linux backends are written but partial and have not passed the full conformance suite. The API is still changing and the crate is not published to crates.io (`publish = false`, version `0.0.0`).

## What it does

- **Native windows that host web UIs.** Pages are served from the app's own origin (`https://tpt-app.localhost` on Windows), so permission grants match the real origin of the page.
- **A typed command channel.** Commands are Rust functions. `#[command]` and `commands!` derive their argument and result contracts, and the contract can be exported as a TypeScript client.
- **Secure by default.** Nothing privileged works until the manifest or the code grants it. Grants are per window and per origin, and can require the user's consent.
- **Audit.** Command calls can be written to a hash-chained audit log. See `tpt-shell-audit`.
- **Kiosk, devices and agents** are opt-in: signed kiosk configs, serial/USB/printer access through a scoped device hub, and an MCP surface for AI agents behind the `agent` feature.
- **No telemetry, no network by default.** The shell opens no sockets unless a feature turns one on.
- **Licence-clean.** Dependencies must be MIT, or dual-licensed with MIT. The policy is enforced with `cargo-deny`.

It does not use wry, tao, winit or Tauri. It talks to WebView2 (COM), WKWebView (Objective-C runtime) and WebKitGTK (loaded with `dlopen`) directly.

## Quick start

On Windows, with WebView2 installed (it is on most current Windows 10 and 11 machines), this opens one window with one embedded page:

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

The `loader` call takes the path to `WebView2Loader.dll`. The `hello` example in this repository runs the same code:

```sh
cargo run -p tpt-shell --example hello -- <path to WebView2Loader.dll>
```

## Main types

| Item | What it is |
|---|---|
| `App` | The entry point. Collects windows, grants, commands, the profile and the audit log, then runs them. |
| `Window` | One native window and the page it shows. |
| `Scope` | The scope a grant applies to: paths, devices, URLs and size limits. |
| `Grant`, `Engine`, `ConsentPrompt` | Re-exported from `tpt-shell-capabilities`. |
| `Router`, `Caller`, `Target`, `Limits`, `Value` | Re-exported from `tpt-shell-ipc`. |
| `AgentTool` | Re-exported from `tpt-shell-agent` (feature `agent`). |
| `Policy` | The audit policy, from `tpt-shell-audit`. |
| `ProfileKind` | Default, named or ephemeral profile. From `tpt-shell-profiles`. |
| `devices` | The device crate, re-exported as a module. |
| `Error`, `Result` | The shell's error type. Every error has a stable `TS####` code. |

The `#[command]`, `commands!`, `register_commands!` and `embed_dir!` macros are re-exported from `tpt-shell-macros`.

## Features

| Feature | Default | What it enables |
|---|---|---|
| `agent` | off | MCP over stdin and stdout for an enabled `[agent]` profile (`tpt-shell-agent`). |
| `admin-endpoint` | off | Token-protected admin endpoint on `127.0.0.1` for kiosk apps. |
| `health-endpoint` | off | Local HTTP health endpoint on `127.0.0.1` for kiosk apps. |
| `devtools` | off | The developer overlay (Ctrl+Shift+D). Intended for debug builds: a release build without it has no overlay page. |

The admin and health endpoints are off by default, so a default build opens no socket.

## Platform support

| Platform | State |
|---|---|
| Windows 10 1809+ and Windows 11, x86_64 | Built and tested on Windows 11. Needs the WebView2 Evergreen runtime or a Fixed Version runtime. |
| macOS 12+ (WKWebView) | Written and type-checked for macOS. Not yet run on a Mac. |
| Linux (GTK 3 with WebKit2GTK 4.1, or GTK 4 with WebKitGTK 6.0) | Written and type-checked. Runs only in CI's adapter jobs. Wayland and X11 are not verified on real desktops. |

[docs/src/platform-notes.md](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/platform-notes.md) has the current detail for each platform and the checklist behind it.

## Support tags

Every public item carries a support tag: **Full**, **Partial** (with the gap stated) or **Unsupported** (returns `Error::Unsupported`). Read the item's documentation before you depend on it.

## Related crates

The workspace is split into one crate per concern. The ones an app author meets most often:

- `tpt-shell-core`: traits (`backend`, `Clock`, `Rng`), geometry and the error type.
- `tpt-shell-ipc`: the IPC envelope, the router and rate limits.
- `tpt-shell-capabilities`: the permission engine.
- `tpt-shell-manifest`: parses and checks `tpt-shell.toml`.
- `tpt-shell-macros`: `#[command]` and friends.
- `tpt-shell-cli`: the `tpt-shell` command line tool.

## Documentation

- [Quick start](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/quickstart.md)
- [Concepts](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/concepts.md)
- [Security](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/security.md)
- [Troubleshooting](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/troubleshooting.md)
- [Design specification](https://github.com/tpt-solutions/tpt-shell/blob/master/spec.txt)

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of

- [MIT licence](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT)
- [Apache licence, version 2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE)

at your option. Third-party components are listed in [THIRD_PARTY_NOTICES](https://github.com/tpt-solutions/tpt-shell/blob/master/THIRD_PARTY_NOTICES).

## Contributing and security

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md). Report vulnerabilities privately through [SECURITY.md](https://github.com/tpt-solutions/tpt-shell/blob/master/SECURITY.md).
