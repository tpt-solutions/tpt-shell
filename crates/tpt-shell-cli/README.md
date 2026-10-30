# tpt-shell-cli

The `tpt-shell` command line tool for [tpt-shell](https://github.com/tpt-solutions/tpt-shell), a
webview app shell for Rust. It scaffolds projects, checks manifests and contracts, runs and
packages apps, and reads audit logs.

The crate is not published yet (`publish = false` until the v1.0.0 release). Build it from the
workspace:

```sh
cargo build -p tpt-shell-cli
```

## Commands

Project setup:

- `tpt-shell new vanilla <dir>` and `tpt-shell new typescript <dir>` scaffold a web front end.
- `tpt-shell new rust <dir>` and `tpt-shell new --template <name> <dir>` scaffold a Rust app.
  Templates are `pos`, `kiosk`, `audit-app`, `agent-demo` and `appfront-dom`.
- `tpt-shell init [dir]` adds tpt-shell to an existing crate.
- `tpt-shell add <feature> [manifest]` adds a feature to `tpt-shell.toml`.

Build and run:

- `tpt-shell run [dir] [--devtools]` builds and launches an app.
- `tpt-shell build [dir]` makes a release build.
- `tpt-shell dev <dir> [--port <n>]` serves a page for a browser tab, with a mock `window.tpt`.
- `tpt-shell doctor [--fix]` checks the WebView2 loader, the project manifest, the licence policy
  and, on Linux, the GTK and WebKit libraries.

Manifests and contracts:

- `tpt-shell manifest check <file>` validates `tpt-shell.toml` with line-accurate errors.
- `tpt-shell explain <code>` prints the explanation and fix hint for a `TS####` error code.
- `tpt-shell contract gen <contract.json> <dir>` writes a TypeScript client.
- `tpt-shell contract diff <old> <new>` and `tpt-shell contract check <new> --baseline <old>`
  report breaking changes.
- `tpt-shell permissions suggest <contract.json>` or `... --audit <log>` proposes capability
  grants.

Audit, replay and release:

- `tpt-shell audit verify | export | view | explain` works on an audit log.
- `tpt-shell replay` and `tpt-shell report` open replays and bug-report bundles.
- `tpt-shell visual compare` runs the visual comparer.
- `tpt-shell compliance <dir> [--key <file>]` writes the SBOMs, notices and attestation.
- `tpt-shell vendor`, `tpt-shell runtime` and `tpt-shell docs` handle vendoring, the pinned
  WebView2 runtime and offline docs.
- `tpt-shell kiosk sign | init` manages the signed kiosk config.
- `tpt-shell install-service <name> <program>` and `uninstall-service <name>` manage the
  per-user sign-in entry. Windows is the only platform with one so far.

Run `tpt-shell` with no arguments to see the usage text.

## Licence

MIT OR Apache-2.0, at your option. Copyright (c) 2026 TPT Solutions.
