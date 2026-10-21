# Kiosk deployment

This guide is for operators who put an app on a kiosk, a shared terminal or a fixed screen. It
says what the shell does, what the operating system must do, and what is not built yet.

Status: the signed config, the state snapshot, the idle timer, the heartbeat and the supervisor
exist. Full-screen and single-window placement, the navigation allowlist and most lock-down
features are not applied by the backends yet. Read the table at the end before you promise a
behaviour to a customer.

## What a kiosk is here

A kiosk is an app that runs with a fixed home page, returns to it after a period of no use, and
recovers from a crash. The shell's own part is small. The rest is the operating system's job.

## The signed config

A kiosk config is a JSON file. The app loads it only if a detached Ed25519 signature next to it
checks out against a public key the app was built or installed with. An edited config is refused,
and the app starts without it.

```json
{
  "mode": "single-app",
  "home": "index.html",
  "idle_seconds": 120,
  "allow": ["https://example.com/help"],
  "fullscreen": true,
  "single_window": true,
  "disable": { "devtools": true, "context_menu": true, "zoom": true }
}
```

Keys (from `tpt-shell-kiosk::policy`):

- `mode`: `"single-app"` (the default for a kiosk config) or `"browser"`.
- `home`: a path under the app origin. Default `index.html`. A path that leaves the app is refused.
- `idle_seconds`: a whole number above 0. Absent means no idle reset.
- `allow`: extra URLs the windows may open. Entries are absolute URLs or app paths.
- `fullscreen` and `single_window`: window placement. The backends do not apply them yet.
- `disable`: booleans for devtools, context menu, zoom, text selection, drag and drop, print
  shortcuts, function keys, swipe gestures and key chords. Unset keys take the mode's default.

The signature is the hex text of a 64-byte Ed25519 signature over the exact bytes of the config
file. The public key is 32 bytes, as hex. The shell reads the signature file, trims it, and
verifies the config bytes before it parses them.

The repository has no signing command yet. The helper `tpt_shell_kiosk::config::sign` takes a
32-byte secret key and returns the hex signature. Call it from a small program that depends on the
`tpt-shell-kiosk` crate by path. Keep the secret key off the kiosk device.

Load the config in the app:

```rust,compile-check
use std::path::Path;
use tpt_shell::{App, Window};

fn build(public_key_hex: &str) -> tpt_shell::Result<App> {
    App::new()
        .name("kiosk-demo")
        .loader("WebView2Loader.dll")
        .asset("index.html", "text/html", "<!doctype html><title>kiosk</title>")
        .window(Window::new("main").title("Kiosk"))
        .kiosk_config(
            Path::new("kiosk.json"),
            Path::new("kiosk.json.sig"),
            public_key_hex,
        )
}
```

`kiosk_config` returns an error when the signature fails, the file is not JSON, or a setting is out
of range. Handle the error. Do not ignore it and run the app without the kiosk rules.

## Idle reset

With `idle_seconds` set, the windows go back to `home` after that long with no page messages. This
is wired on Windows. Clearing the session and the ephemeral profile on idle is not built (todo
M6). Pair the idle reset with an ephemeral profile only after testing it on your build.

## State snapshot

`App::snapshot_state(path, every, capture, restore)` saves what `capture` returns to a JSON file at
an interval and when the event loop ends. At the next start, if the file exists, `restore` gets
its value once before the windows show. A file that cannot be read is reported on standard error
and skipped.

The snapshot is stored as plain JSON. Do not put secrets in it.

## Crash recovery and the supervisor

`tpt-shell-supervisor` is a separate program. It starts the app, watches a heartbeat file the app
rewrites (passed in `TPT_HEARTBEAT_FILE`), and restarts the app after a crash or a missed heartbeat,
with backoff. A crash loop stops the restarts. It can run a scheduled restart (for example
`--restart-at 03:00`), an alert command or an alert file, and write a log. Its memory use is
measured on Windows only.

Page crash recovery (reload the last good page) is not built (todo M6).

## Operating system lock-down

The shell cannot lock the desktop from user space. Use the operating system:

- **Windows:** Assigned Access (kiosk mode) in Settings, or `Set-AssignedAccess` with the user name
  and the app's package family name, on Windows Pro or Enterprise. Ctrl+Alt+Del and the power menu
  stay available. Disable them by policy if the device needs it.
- **macOS:** Guided Access for a single session, or an MDM profile that controls the Dock, Login
  Items and allowed apps. A LaunchAgent starts the supervisor at login. Not tested on macOS yet.
- **Linux:** a dedicated session that starts only the app, for example an autologin account whose
  session runs the supervisor and the app as a systemd user unit, with the window manager's own key
  bindings removed. Not tested on Linux yet.

The Windows key-chord hook blocks Alt+Tab, Win, Alt+F4 and Ctrl+Esc on a best-effort basis. Ctrl+Alt+Del
is handled by Windows and cannot be blocked from the app.

## What is built

| Behaviour | Status |
|---|---|
| Signed config is verified before use | Built, tested |
| Home page and idle reset | Built on Windows; clearing session and profile on idle is not |
| Full-screen and single-window placement | Parsed; backends do not apply it yet (todo M6) |
| Navigation allowlist | Parsed; enforcement is partial (see [Security](../security.md)) |
| Lock-down features (devtools, context menu, zoom and so on) | Parsed; only some are applied, on Windows (todo M6) |
| Key-chord hook | Windows only, best effort |
| Multi-monitor placement and rotation | Not built |
| Local admin endpoint and health endpoint | Behind the `admin-endpoint` and `health-endpoint` features, off by default. Not in the todo as done |
| Signing tool for configs | Not built as a command (helper function only) |
| `tpt-shell add kiosk` and the `kiosk` template | Not built (todo M6, M11) |
| macOS and Linux | Not built on those platforms; cross-compiled only |

Do not describe a kiosk built on this shell as locked down until you have tested your own image on
the target hardware.
