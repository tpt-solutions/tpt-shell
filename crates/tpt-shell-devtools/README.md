# tpt-shell-devtools

The developer overlay for tpt-shell apps. It shows an app's IPC log with latency, its jobs, the capabilities in effect, the audit tail, a contract explorer that can call commands, and engine information.

It is for debug builds. A release build gets no overlay page.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## How it works

- The overlay reads shell state through the `Inspect` trait. The app implements it. This crate never reaches into the runtime itself.
- `Snapshot` is a plain, owned copy of that state.
- `page` turns a snapshot into one HTML file with no external resources. The app opens it in a window.
- The host refreshes the page by calling `window.tptDevtools.update` with a new snapshot, as JSON, each time the overlay is shown. The facade does this in `crates/tpt-shell/src/devtools.rs`.
- The page calls `window.tpt.invoke` only when a developer presses Call in the contract explorer.

Nothing here opens a socket or sends data anywhere.

## Enabling it

`enabled()` is `true` only in debug builds. `page_if_enabled` returns `None` in a release build, so the overlay cannot be opened there.

The facade turns the overlay on with its `devtools` feature. Keep that feature off in release builds:

```sh
cargo run --features devtools
```

The overlay opens with Ctrl+Shift+D.

## Modules and types

| Item | Contents |
|---|---|
| `Snapshot` | The owned state: `Engine`, IPC entries, `Capability` rows, `Job` rows, `CommandInfo` and `AuditRow`. |
| `Inspect` | The trait the app implements to supply the snapshot. |
| `page`, `page_if_enabled` | Build the overlay HTML. |
| `audit_rows` | Reads the last audit lines (up to `AUDIT_TAIL`, 200) from a JSON Lines file. |
| `timeline` | Timeline data for the overlay. |

## Support

| Part | State |
|---|---|
| Snapshot, audit-line reader and page | Full, in debug builds on Windows. |
| Jobs panel | Partial. The facade runs no jobs, so the panel is empty. |
| Engine version | Partial. Not reported. |

## Features

| Feature | Default | What it enables |
|---|---|---|
| `overlay` | on | The overlay page and its HTML. The facade builds with it off and enables it only through its own `devtools` feature. |

## Platform support

The page is HTML and runs on any backend that has a page. Tested on Windows in debug builds.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
