# tpt-shell-supervisor

The restart policy for a supervised tpt-shell app. It runs the app as a child process, watches a heartbeat, restarts it with backoff, gives up after a crash loop, raises an alert, and keeps a rotating log.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The decisions are tested with fault injection. The platform install functions have not been run on every target.

## How it decides

The decisions are pure functions of the times they are given. They can be tested without waiting for a clock.

| Type | Decision |
|---|---|
| `Backoff` | The wait before a restart. It grows exponentially and is capped. |
| `CrashLoopGuard` | Counts crashes inside a time window. Too many means stop and alert. |
| `Heartbeat` | A hang is a missed heartbeat past a timeout. The app writes the heartbeat file (see `tpt-shell-kiosk`). |
| `Scheduled` | A daily restart at a fixed time of day. |
| `Decision` | What the loop should do next: wait, restart, stop or alert. |

`supervise` runs the loop over a `Spawner`. The platform implements `Spawner` with real processes.

## Alerts and logs

When the supervisor gives up, it writes an alert. An alert can run a command or write a file. It also keeps a rotating log of each start, exit and restart.

## Install

The `install` module writes the start-at-login entry for the current user:

| Platform | Entry |
|---|---|
| Windows | A Task Scheduler task. |
| macOS | A LaunchAgent. |
| Linux | A systemd user unit. |

The command line tool writes these files with `tpt-shell install-service <name> <program>` and removes them with `uninstall-service`. On macOS and Linux it writes the files and prints the commands to run, and does not run them.

## Usage

```rust
use std::time::Duration;
use tpt_shell_supervisor::CrashLoopGuard;

// Stop and alert after 5 crashes in 10 minutes.
// The backoff argument comes from `Backoff`.
```

The loop is driven by `supervise`, which takes a `Spawner`. Most users run the app through the command line tool or the install entry, not by calling these types directly.

## Platform support

| Part | Windows | macOS | Linux |
|---|---|---|---|
| Restart policy, heartbeat, alerts, log | Platform-neutral, tested | Platform-neutral, tested | Platform-neutral, tested |
| Process spawning | Built on Windows (`tpt-shell-platform-windows`) | Not run | Not run |
| Start-at-login entry | Task Scheduler | LaunchAgent, files only | systemd user unit, files only |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
