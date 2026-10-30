# tpt-shell-kiosk

Kiosk support for tpt-shell. It covers a signed local config, state snapshots that restore after a restart, an idle timer that resets the app, page crash recovery, the key chords a kiosk blocks, and the heartbeat that a supervisor watches.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The parts that do not need a window are built and tested. Full lock-down is not built, and the OS does most of it (see below).

## What it provides

| Module | Contents |
|---|---|
| `config` | A kiosk config. It applies only when its Ed25519 signature verifies. |
| `policy` | The values a signed config sets: single-app mode, the navigation allow-list, full-screen and single-window placement, and the list of disabled features. The backends apply them. |
| `state` | A state snapshot. Saved on an interval and at exit, and restored at the next start. Writes are atomic. |
| `idle` | An idle timer that sends the kiosk back to its home page. |
| `recovery` | Page crash recovery. It reloads the last good page. |
| `chords` | The key chords a kiosk blocks: Alt+Tab, Win, Alt+F4 and Ctrl+Esc. |
| `heartbeat` | The heartbeat file that `tpt-shell-supervisor` watches. It is written only when the app is supervised. |
| `admin` | Local admin endpoint on `127.0.0.1`, token protected. Feature `admin-endpoint`. |
| `health` | Local HTTP health endpoint on `127.0.0.1`. Feature `health-endpoint`. |
| `http` | The small HTTP server the two endpoints share. |
| `hook_windows` | The Windows low-level keyboard hook that blocks chords. Best effort. |

## Features

| Feature | Default | What it enables |
|---|---|---|
| `admin-endpoint` | off | The admin endpoint. It opens a socket. |
| `health-endpoint` | off | The health endpoint. It opens a socket. |

With both off, a default build opens no socket.

## Signed config

A kiosk config is not trusted until its signature is checked against a public key. The `tpt-shell kiosk init` command writes a starter config and `tpt-shell kiosk sign` signs one after checking it.

## Lock-down is done by the OS

A kiosk app cannot lock the whole desktop from user space. Use the operating system for that, and let the app handle what it can:

- **Windows:** Assigned Access (kiosk mode) in Settings, or `Set-AssignedAccess` on Windows Pro or Enterprise. Ctrl+Alt+Del and the power menu stay available, so disable them by policy if the device needs it.
- **macOS:** Guided Access for one session, or an MDM profile that sets the Dock, Login Items and allowed apps. The supervisor's install code can write a LaunchAgent to start it at login.
- **Linux:** a dedicated session that starts only the app, for example an autologin account.

## Usage

```rust
use tpt_shell_kiosk::chords;
```

Kiosk support is wired into the facade through `App::kiosk_config` and `App::snapshot_state`. Most apps use those and do not call the modules directly.

## Platform support

| Part | Windows | macOS | Linux |
|---|---|---|---|
| Config, state, idle, recovery, heartbeat | Built and tested | Platform-neutral code. Compiles, not run on macOS. | Platform-neutral code. Compiles, not run on Linux. |
| Key chord hook | Built, best effort | Not built | Not built |
| Full lock-down and placement | Not fully built (see `todo.md` M6) | Not built | Not built |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
