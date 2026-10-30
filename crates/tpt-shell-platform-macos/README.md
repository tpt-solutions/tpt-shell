# tpt-shell-platform-macos

The macOS backend for tpt-shell: AppKit windows and WKWebView pages, built with the generated `objc2` bindings. The code is compiled only on macOS.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: written, not yet run on a Mac.** The workspace type-checks this crate for the macOS target from Windows. Its runtime tests run on macOS CI and skip with a message when there is no GUI session. Not published (`publish = false`, version `0.0.0`).

## What is written

| Area | Module | State |
|---|---|---|
| Windows | `window` | Frameless, transparent, always-on-top, vibrancy backdrop and close action. |
| WKWebView | `webview` | Message bridge between the page and Rust. |
| Document-start scripts | `shim` | Scripts injected before the page's own script runs. |
| Navigation policy | `policy` | The rules that decide where a page may go. These compile and are tested on every host. |
| App scheme | `webview` | `app://` scheme handler that serves the app's files. |
| Snapshots | `backend` | Window snapshots. |
| Dialogs | `dialogs` | Message boxes and file dialogs. |
| Clipboard | `backend` | Pasteboard text. |
| Appearance | `theme` | Observes the system appearance. |
| Menu bar tray | `tray` | Menu bar item. |
| Single instance | `single_instance` | By bundle identifier. |
| Keychain | `keystore` | Security framework, through `objc2-security`. Licence Zlib OR Apache-2.0 OR MIT, used under MIT. |

## What is not built

- The event loop.
- Notifications.
- Deep links.
- Accessibility.

## Profile isolation

WKWebView profile isolation needs macOS 14 or later. macOS 12 and later can run the window and webview code.

## Testing

`tests/runtime.rs` runs on macOS CI. It uses `harness = false`, because AppKit must run on the process main thread, which the default test harness does not use. Each check skips with a message when there is no GUI session. The policy tests run on every host.

## Usage

The backend is used through the facade. An app does not call this crate directly.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
