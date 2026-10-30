# tpt-shell-platform-windows

The Windows backend for tpt-shell. It opens Win32 windows that host WebView2 pages, with the platform's own COM bindings, and provides the Windows parts of the shell: consent and message dialogs, the keystore, the tray icon, single instance, deep links, window placement, serial ports, printing, the kiosk key hook and UI Automation.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: the Windows backend is the one verified to open windows.** It was built and tested on Windows 11. Not published (`publish = false`, version `0.0.0`). Some features listed below are still partial or not built.

## Requirements

- Windows 10 1809 or later, or Windows 11, on x86_64.
- The WebView2 runtime: the Evergreen runtime (installed with most current Windows machines) or a Fixed Version runtime pinned with `tpt-shell runtime pin`.
- `WebView2Loader.dll`, which the app loads through `App::loader`. The loader is BSD-3-Clause and is listed in `THIRD_PARTY_NOTICES`.

## Unsafe code

All `unsafe` code and FFI for Windows lives in this crate's platform modules, each with its own tests. The rest of the workspace is safe Rust. On other operating systems this crate compiles to nothing.

## What is built

| Area | Modules | State |
|---|---|---|
| Windows and WebView2 | `backend`, `webview2`, `handler`, `event_handler`, `shim`, `com`, `loader` | Built and tested. Each window loads the app origin under the page policy. |
| Page policy | `placement`, `profile` | Strict Content-Security-Policy on every shell page. Developer tools and the context menu are off in release builds. Navigation is limited to `allow_navigation` prefixes. |
| Permissions | `consent`, `dialogs` | Grants that need consent ask in a native message box owned by the requesting window. The default answer is No. Multi-select open dialog. |
| Agent confirmation | `agent_confirm` | Native Yes/No dialog for agent actions. |
| Kill switch | `kill_hotkey` | Ctrl+Alt+Shift+K, Partial. |
| Keystore | `keystore`, `random` | DPAPI, per user, one encrypted file per name. Holds the audit checkpoint key. |
| Tray and notifications | `tray` | Notification-area icon, left-click reports `TRAY_ACTIVATE`, right-click menu, balloon notifications. |
| Single instance | `single_instance` | Named mutex and `WM_COPYDATA`. A later launch hands its arguments to the first instance and exits. |
| Deep links | `deep_link`, `registry` | Registers a URL scheme for the app. |
| Window state | `placement` | Window placement is saved and restored, and off-screen windows are brought back. |
| Clipboard and time | `clipboard`, `local_time` | Clipboard text and local time for pages. |
| Start at sign-in | `startup` | Per-user Run entry, written by `tpt-shell install-service`. |
| Accessibility | `accessibility`, `uia`, `native_tree` | UI Automation dumps for the test driver. |
| Serial and printing | The Windows transports in `tpt-shell-devices` | Serial ports on COM1 to COM999, and raw spooler printing through winspool. |
| Kiosk | The key hook in `tpt-shell-kiosk` (`hook_windows`) | The key chord hook is best effort. |

Visual effects: transparent windows, drag regions (Partial), dark title bar, Mica, Acrylic and Tabbed backdrops, and the colour scheme driven through WebView2. High contrast, reduced motion and reduced transparency reach the page as `data-` attributes, and open pages update them when the user changes a setting.

## What is not built

- Frameless and always-on-top windows: listed as open work in `todo.md`.
- WebView2 channels (stable, beta, canary) for the visual diff.
- Per-window profiles. All windows share one folder for now.
- Full kiosk placement and lock-down (`todo.md` M6).
- HID and USB raw access (`todo.md` M7).

## Usage

The backend is used through the facade. An app does not call this crate directly:

```rust
use tpt_shell::{App, Window};

App::new()
    .name("my-app")
    .loader("WebView2Loader.dll")
    .window(Window::new("main").title("My app"))
    .run()?;
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
