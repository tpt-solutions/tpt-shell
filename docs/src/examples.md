# Examples gallery

This page lists the examples that exist in the repository, and the ones that are planned. Only the
first group can be run.

## Available

| Example | Where | What it shows | Platform |
|---|---|---|---|
| `hello` | `crates/tpt-shell/examples/hello.rs` | One window, one embedded page, single instance | Windows |

Run it from a checkout, with the path to `WebView2Loader.dll`:

```sh
cargo run -p tpt-shell --example hello -- path\to\WebView2Loader.dll
```

The integration test `crates/tpt-shell/tests/commands.rs` is also a worked example of a typed command
with a grant and a scope. It is not run as an app.

## Templates

The `tpt-shell new` command creates a starter project from a template. These exist:

- `vanilla`: one HTML page and a manifest. `tpt-shell new vanilla my-app`.
- `typescript`: a page with a TypeScript source file and a typed runtime declaration.
  `tpt-shell new typescript my-app`.
- `rust`: a Cargo project that opens one window. `tpt-shell new rust my-app`. Its path
  dependency on tpt-shell points at the checkout the CLI was built from; pass
  `--tpt-shell <path to crates/tpt-shell>` to use another.
- The Rust starters `pos`, `kiosk`, `audit-app`, `agent-demo` and `appfront-dom`:
  `tpt-shell new --template pos my-app`.

The `vanilla` and `typescript` projects are pages only. Build a window around them with the
`rust` starter or the [Quickstart](quickstart.md). `tpt-shell run` builds and launches a Rust
project.

## Planned

The following are on the todo list in M11 and in the milestones that define them. None of them
exists in `examples/` yet, so do not link to them as working code.

| Planned | Purpose | Todo |
|---|---|---|
| `kiosk` | A signed kiosk config and a home page | M6, M11 |
| `pos` | A point-of-sale page with receipt printing to a virtual printer | M7, M11 |
| `audit-viewer` | Reading and viewing an audit log | M9, M11 |
| `agent-demo` | An agent completing a scripted task under a policy | M9, M11 |
| `devices-sim` | Virtual serial, HID, printer and scanner devices | M7, M11 |
| rustvac port | The rustvac app rewritten as an HTML UI on tpt-shell | M11 |

Planned templates: `appfront-dom`, `kiosk`, `pos`, `agent-demo` and `audit-app`. The `kiosk` template
is a preview, finalised in M11.

Each planned example needs its own review under the docs rules before it is listed as available.
