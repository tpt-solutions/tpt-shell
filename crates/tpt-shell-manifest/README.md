# tpt-shell-manifest

The parser and validator for `tpt-shell.toml`, the manifest that declares an app's name, windows, capabilities, audit settings and optional sections.

Every error has a stable `TS####` code and, where the problem is in the text, the line it is on. Unknown keys are rejected in the sections this phase owns.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## What it checks

- Syntax, with the line of the error.
- Unknown keys, which are rejected.
- Values, such as window sizes and URL prefixes.
- The `[audit]` section (`store_params` and `redact`), validated against the published schema in `schema/`.

Sections for later phases are accepted and kept as raw TOML: `kiosk`, `devices`, `agent`, `replay`, `test`, `profiles` and `compliance`. Each of those has its own parser, which arrives with its phase. They are returned in `Manifest::stubs`.

## Usage

```rust
use tpt_shell_manifest::parse;

let text = std::fs::read_to_string("tpt-shell.toml").unwrap();
match parse(&text) {
    Ok(manifest) => println!("{} window(s)", manifest.windows.len()),
    Err(e) => eprintln!("{} line {:?}: {}", e.code, e.line, e.message),
}
```

`Manifest` holds:

- `app`: the app's settings.
- `windows`: the windows to open.
- `capabilities`: the capability names the app asks for.
- `audit`: the audit settings.
- `stubs`: the raw sections for later phases.

The `codes` module lists every error code this crate can return.

The command line tool checks a file with `tpt-shell manifest check tpt-shell.toml`.

## Platform support

None. The parser is platform-neutral.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
