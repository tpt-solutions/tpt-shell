# tpt-shell-errors

The registry of stable `TS####` error codes for tpt-shell. Each entry has a code, a short title, a longer explanation and a fix hint.

Every error that tpt-shell reports carries one of these codes. Codes never change meaning once they are published. A code that is retired keeps its number.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## Usage

```rust
use tpt_shell_errors::lookup;

let title = lookup("TS0001").map(|entry| entry.title);
assert_eq!(title, Some("manifest syntax error"));
```

`lookup` ignores case and surrounding whitespace in the code.

From the command line, `tpt-shell explain <TS####>` prints the entry:

```sh
tpt-shell explain TS0001
```

## What is in the registry

Each `Entry` is a `&'static` value in `REGISTRY`:

```rust
pub struct Entry {
    pub code: &'static str,
    pub title: &'static str,
    pub explanation: &'static str,
    pub fix: &'static str,
}
```

The ranges in use:

| Range | Area |
|---|---|
| `TS0001` to `TS0004` | Manifest syntax, unknown keys and invalid values. |
| `TS0100` to `TS0122` | Audit, agent, and the licence gate. |
| `TS0301` to `TS0305` | The test driver. |

The full list is the `REGISTRY` constant in `src/lib.rs`.

## Adding a code

Pick the next free number in the right range, and never reuse a number. Every code needs an explanation and a fix hint. The project's review checklist requires both.

## Platform support

None. The registry is data.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
