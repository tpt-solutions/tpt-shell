# Examples gallery

This page lists the examples and starter templates in the repository. Each one is a separate
project outside the Cargo workspace, so run it from its own folder. They depend on the crates in
this checkout by path.

Two kinds of example:

- **Window examples** open a WebView2 window. They run on Windows only, and need the WebView2
  loader (`WebView2Loader.dll`) for the first run. Pass its path as the argument, or set
  `TPT_WEBVIEW2_LOADER`.
- **Headless examples** print their results and open no window. They run on any OS.

## Examples

Each example folder under `examples/` holds a `Cargo.toml` and a `src/main.rs`. Its page, if
any, is in `web/`.

| Example | Kind | What it shows | Run it |
|---|---|---|---|
| `agent-demo` | Headless | An agent session under a policy: a normal call runs and is logged, a call with bad arguments is refused before the handler runs, and a sensitive call asks the user (the example declines). | `cd examples/agent-demo` then `cargo run` |
| `devices-sim` | Headless | Virtual receipt printer, barcode scanner, HID device and serial loopback, behind one device hub, with the audit trail the hub writes. | `cd examples/devices-sim` then `cargo run` |
| `audit-viewer` | Window | Checks an audit log's hash chain and shows its last entries. The page says "chain ok" only when the chain verifies. | `cd examples/audit-viewer` then `cargo run -- <audit.jsonl>` |
| `kiosk` | Window | A kiosk window whose home page and idle reset come from a signed config. The config is applied only if its Ed25519 signature checks out. | `cd examples/kiosk` then `cargo run` |
| `pos` | Window | A till whose items are totalled by the host, with a receipt written to a virtual ESC/POS printer through the device hub. | `cd examples/pos` then `cargo run` |

The integration test `crates/tpt-shell/tests/commands.rs` is also a worked example of a typed
command with a grant and a scope. It is a test, not an app.

The hello-world example in the facade crate is a minimal window program:

```sh
cargo run -p tpt-shell --example hello -- path\to\WebView2Loader.dll
```

## Templates

`tpt-shell new` creates a starter project from a template. Templates are the starting point for
your own app; examples show a finished behaviour.

- `vanilla`: one HTML page and a manifest. `tpt-shell new vanilla my-app`.
- `typescript`: a page with a TypeScript source file and a typed runtime declaration.
  `tpt-shell new typescript my-app`.
- `rust`: a Cargo project that opens one window. `tpt-shell new rust my-app`. Its path dependency
  on tpt-shell points at the checkout the CLI was built from; pass `--tpt-shell <path to
  crates/tpt-shell>` to use another.
- `pos`, `kiosk`, `audit-app`, `agent-demo` and `appfront-dom`:
  `tpt-shell new --template pos my-app`. Each one is a Rust project copied from `templates/`,
  with its path dependencies pointed at the checkout.

The `vanilla` and `typescript` projects are pages only. Build a window around them with the `rust`
starter or the [Quickstart](quickstart.md). `tpt-shell run` builds and launches a Rust project.

## Planned

Not yet in the repository, so do not link to them as working code:

| Planned | Purpose | Todo |
|---|---|---|
| rustvac port | The rustvac app rewritten as an HTML UI on tpt-shell. Needs the rustvac project, which is not in this repository. | M11 |
