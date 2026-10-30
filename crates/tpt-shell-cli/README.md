# tpt-shell-cli

The `tpt-shell` command line tool. It explains error codes, checks manifests, scaffolds and runs projects, diffs command contracts, audits and replays runs, writes compliance outputs, and installs the start-at-login entry.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). Run it from a checkout of the repository.

## Running it

From the root of the repository:

```sh
cargo run -p tpt-shell-cli -- <command>
```

Or build it once and call the binary:

```sh
cargo build -p tpt-shell-cli --release
target/release/tpt-shell <command>
```

Exit codes: `0` for success, `1` when the checked input has problems, and `2` for bad usage or an I/O failure.

The crate is not published yet (`publish = false`). It is built from the workspace, and `cargo install --path crates/tpt-shell-cli` installs the binary from a checkout.

## Commands

| Command | What it does |
|---|---|
| `explain <TS####>` | Prints the registry entry for an error code. |
| `manifest check <tpt-shell.toml>` | Checks a manifest and prints each problem with its code and line. |
| `new <vanilla\|typescript\|rust> <dir>` | Creates a starter project. `new --template <name> <dir>` uses one of `pos`, `kiosk`, `audit-app`, `agent-demo` or `appfront-dom`. |
| `init [dir]` | Adds tpt-shell to an existing Rust crate. |
| `add <kiosk\|devices\|audit\|agent> [tpt-shell.toml]` | Writes that section into the manifest. It refuses a section that already exists. |
| `dev <dir> [--port N]` | Serves an app's files on `127.0.0.1` for a browser tab, with a mock `window.tpt`, and reports which files change. |
| `build [dir]` | Release build, with `cargo build --release --locked`. |
| `run [dir]` | Builds and launches the Rust project. |
| `doctor [--fix]` | Checks the WebView2 loader, the manifest, the licence policy and, on Linux, the GTK and WebKit libraries. `--fix` installs `cargo-deny` when it is missing. |
| `contract diff <old> <new>` | Lists each change as breaking or compatible. |
| `contract check <new> --baseline <old>` | Lists only the breaking changes, and exits 1 when there are any. |
| `contract gen <contract.json> <out>` | Writes the TypeScript client, `commands.ts`. |
| `permissions suggest <contract.json>` | Suggests the grants the commands need. |
| `permissions suggest --audit <log>` | Suggests the grants that refused calls were missing. |
| `kiosk init <dir> --key <file> [--install <program>]` | Writes a starter signed kiosk config. |
| `kiosk sign` | Signs a kiosk config after checking its signature. |
| `audit verify <audit.jsonl> [--key] [--anchors]` | Checks the hash chain, checkpoint signatures and anchors. |
| `audit verify <dir> --rotated <stem>` | Checks rotated audit segments. |
| `audit export <audit.jsonl> --format csv\|jsonl` | Exports a verified log. |
| `audit view <audit.jsonl>` | Prints a short table of a verified log. |
| `audit explain <audit.jsonl>` | Prints a plain-language summary. |
| `replay <file.tptreplay> [--strict\|--lenient]` | Plays a recording against the echo reference target. |
| `report <out.json> [--replay] [--log] [--audit] [--version] [--redact]` | Writes a bug-report bundle, with redaction. |
| `visual compare <baseline> <dir>... [--out] [--tolerance] [--max-changed]` | Compares screenshot folders and writes an HTML report. |
| `compliance <out-dir> [--key <file>]` | Writes the SBOMs, attestation and notices, and signs them with `--key`. It exits 1 on a refused licence. |
| `runtime pin <tpt-shell.toml> (--cab <file> \| --url <https-url>) --out <dir>` | Pins a WebView2 runtime folder. |
| `runtime list <dir>...` | Lists the pinned runtimes. |
| `vendor [dir]` | Vendors the dependencies. |
| `install-service <name> <program>` | Writes a start-at-login entry for the current user. `uninstall-service` removes it. |

The usage text is printed by running the tool with no arguments.

## Examples

Check a manifest:

```sh
tpt-shell manifest check tpt-shell.toml
```

Look up an error code:

```sh
tpt-shell explain TS0001
```

Check that an API change is safe for a client built against the old contract:

```sh
tpt-shell contract check new-contract.json --baseline old-contract.json
```

## Platform support

| Command group | Windows | macOS | Linux |
|---|---|---|---|
| Manifest, contract, audit, replay, report, compliance, visual | Built and tested | Built, platform-neutral | Built, platform-neutral |
| `install-service` | Writes a per-user Run entry | Writes a LaunchAgent file (prints the command, does not run it) | Writes a systemd user unit (prints the command, does not run it) |
| `doctor` | Built | Built | Built, and checks the GTK and WebKit libraries |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
