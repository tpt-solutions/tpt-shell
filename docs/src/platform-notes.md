# Platform notes

This page says what runs on each operating system today. It follows the checklist in `todo.md`.
Where a box is not ticked, the feature is not built, whatever the code path looks like.

## Summary

| Platform | Built | Tested on the machine | Notes |
|---|---|---|---|
| Windows 10 1809+ and Windows 11, x86_64 | Yes. Windows is the only backend that opens windows | Yes, on Windows 11 | Needs WebView2 Evergreen or a Fixed Version runtime |
| Windows aarch64 | Target listed in the spec. Build not verified here | No | Do not promise it |
| macOS 12+ (WKWebView) | Partly. The window and webview are written and type-check for macOS | No. Cross-compiled only | Profile isolation needs macOS 14+ |
| Linux (GTK 3.24 with WebKit2GTK 4.1, or GTK 4 with WebKitGTK 6.0) | Partly. Both GTK adapters and the adapter selection are written and type-check for x86_64 and aarch64 Linux | Not on the machine. They run only in the GitHub CI adapter jobs (Xvfb and headless Weston), which are non-blocking until green | Wayland and X11 are not verified on real desktops |

Cross-compiled means the code builds for that target from a Windows machine. It does not mean the
code has run there. Do not describe macOS or Linux as working.

## Windows

What is built and tested:

- Windows, with WebView2. Each window loads the app origin, under the page policy.
- The capability-checked command router, with the grants and consent dialog.
- Audited command calls, with `manifest_audit` or `audit`.
- Blobs, with a limit (`blob_limit`, 64 MiB by default). Other platforms have no blob channel.
- Single instance, deep links, the tray icon and the window placement memory.
- Serial ports (`COM1` to `COM999`, 8 data bits, no parity, one stop bit).
- Raw printing through the spooler (winspool RAW).
- The DPAPI keystore for audit signing keys.
- The low-level key hook for kiosk key chords (best effort).
- The Windows CLI install step (`install-service` writes a per-user Run entry).

What is not built on Windows:

- Window features beyond the basics. Frameless, always-on-top and transparency are in the todo.
- WebView2 channels (stable, beta, canary) for the visual diff.
- Full kiosk placement and lock-down features (todo M6).
- HID and USB raw access (todo M7).
- Per-window profiles. All windows share one folder (todo M6).
- Visual and accessibility dumps through UI Automation (todo M8).

The Windows runtime requirement: the Evergreen WebView2 runtime is installed on current Windows 10
and Windows 11 machines. A Fixed Version runtime works too, but the shell does not fetch or pin one
(todo M10).

## macOS

Written, not run:

- An `NSWindow` and `WKWebView` are created and HTML is loaded. The code type-checks against macOS
  bindings. It has not run on a Mac.
- The data-store identifier for profile isolation needs macOS 14 or later.

Not built:

- The script message handler and the custom scheme handler. Pages cannot send commands yet.
- The navigation policy, snapshots, file dialogs, the clipboard, notifications, the status item,
  deep links, single instance, the Keychain and the accessibility tree.
- Serial ports (termios). Calls return `NotSupported`.
- The raw print path is Partial: it goes through CUPS `lp -o raw`, which has not run from this
  workspace.

The macOS minimum version is 12. `doctor` does not check it yet, and the notarisation notes are not
written.

## Linux

Built:

- A dlopen loader for the GTK and WebKit libraries, with a clear error when a library is missing.
  Tested with missing libraries.
- Two window adapters, GTK 3 with WebKit2GTK 4.1 and GTK 4 with WebKitGTK 6.0: window, webview,
  page messages, the `app://` scheme, the navigation policy, web-process crash recovery, snapshot,
  the clipboard and file dialogs. They are written and type-checked from Windows, and have not been
  run by hand. The CI job `linux-adapters` runs each adapter under Xvfb and under headless Weston.
- Adapter selection at start-up: GTK 4 if both load, GTK 3 otherwise. `TPT_GTK=3` or `TPT_GTK=4`
  forces one, and the start fails with a message if the forced adapter is not installed (there is
  no silent fall back). With neither installed the error names the packages to install.
  `tpt-shell doctor` prints which adapter would run.
- The platform-independent crates (audit, devices, replay and others) are written without
  Windows-only code. A Linux build of them has not been verified (the aarch64 and x86_64 Linux
  builds are open in the todo).

Not built:

- Proof that the adapters work. Until the CI jobs are green and a person has run an app on a Linux
  desktop, do not describe the Linux window backend as working. `App::run` is not wired to the
  Linux adapters yet.
- The tray, notifications, keystore (Secret Service), accessibility dump (AT-SPI), and the Wayland
  and X11 checks.
- Serial ports (termios) and HID (hidraw).

Global hotkeys are planned as X11-only. A Wayland session does not allow them.

### Runtime packages

The adapters load GTK and WebKit with `dlopen`, so only the runtime libraries are needed, not the
`-dev` packages. On Ubuntu 24.04:

| Adapter | Packages |
|---|---|
| GTK 3 | `libgtk-3-0t64` (or `libgtk-3-0`), `libwebkit2gtk-4.1-0` |
| GTK 4 | `libgtk-4-1`, `libwebkitgtk-6.0-4` |

GTK 4 file dialogs need GTK 4.10 or later (Ubuntu 22.04 has 4.6, where they return `Unsupported`).
GTK 3 and GTK 4 cannot run in one process; the shell starts exactly one.

### Wayland and X11

GTK picks the display server from the session (`WAYLAND_DISPLAY` first, then `DISPLAY`). Set
`GDK_BACKEND=x11` or `GDK_BACKEND=wayland` to force one. What differs:

| Feature | X11 | Wayland |
|---|---|---|
| Window position and bounds | GTK 3 only. GTK 4 has no position API, so it returns `Unsupported` | The compositor places windows. Position is ignored or `Unsupported` |
| Always on top | GTK 3 only (window manager permitting). `Unsupported` on GTK 4 | Not available to a client. A compositor may ignore the GTK 3 request |
| Global hotkeys | Partial, through XGrabKey | `Unsupported`. A Wayland client cannot grab keys globally |
| Transparency | Needs a compositing window manager. `Unsupported` otherwise | Works when the compositor composites, which they all do |
| Clipboard | Works in-process and with a clipboard manager | The compositor gives the clipboard only to a focused client, so a background or headless run may read nothing |
| Frameless windows | No drag regions yet, so such a window cannot be moved by its background | Same |

None of this has been verified on real desktops. The CI jobs use Xvfb (X11, no compositor) and
headless Weston (Wayland, no input or focus), so they show that the adapters start and draw, not
how a user's desktop behaves. Tray (StatusNotifier) and notifications need D-Bus and the `dbus`
cargo feature.

## What the checklist says about conformance

The conformance suite lives in `conformance/`, and it reports checks that need a platform backend as
pending. Run it with `TPT_CONFORMANCE_REQUIRE_PASS=1` to make pending checks fail. Parity across the
three systems is an open milestone (todo M4 and M5). Until it is done, a feature listed here as
Windows only should be described as Windows only.

## Changing this page

When a todo item is ticked, update the matching row here in the same change. A row may say "built"
only when the todo item is ticked, or when the note says what is still missing.
