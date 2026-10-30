# tpt-shell-core

Core traits, geometry, the error type and the test doubles for tpt-shell. Everything here is OS-independent: no platform code and no FFI.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell). Most apps use the `tpt-shell` facade rather than this crate directly.

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The API is still changing.

## What it provides

| Module | Contents |
|---|---|
| `backend` | The window backend traits that each platform crate implements, and `devtools_policy` and `PreferredScheme`. |
| `platform` | Platform-neutral types and helpers shared by the backends. |
| `clock` | The `Clock` trait, with `SystemClock` and `FakeClock`. The replay system swaps in its own clock. |
| `rng` | The `Rng` trait, with `SystemRng` and `SeededRng`. |
| `executor` | `Executor` and `block_on`, a small executor with no runtime dependency. |
| `geometry` | `LogicalSize`, `LogicalRect`, `PhysicalSize` and `ScaleFactor`. Logical and physical units are separate types, so they cannot be mixed by accident. |
| `navigation` | Navigation decisions for the page policy. |
| `theme` | `Theme`, `Signals` and `injection_script`, which carries colour scheme and accessibility settings into a page. |
| `error` | `Error`, `Result` and `Support`. |
| `fake` | An in-memory backend for unit tests. It opens no window. |

## Support tags

The traits describe what a backend should do. The `Support` value on each backend call says whether the platform does it fully, partly or not at all. Calls a backend does not support return `Error::Unsupported`.

## Usage

Add the crate to a `Cargo.toml`:

```toml
[dependencies]
tpt-shell-core = { version = "0.0.0" }
```

Use the fake clock and RNG in a test, so time and randomness are fixed:

```rust
use tpt_shell_core::{Clock, FakeClock, Rng, SeededRng};

let clock = FakeClock::default();
let mut rng = SeededRng::new(42);
let first = rng.next_u64();
```

The exact constructors are in the module documentation for `clock` and `rng`.

## Platform support

None. This crate is the platform-neutral layer. The backends are `tpt-shell-platform-windows`, `tpt-shell-platform-macos` and `tpt-shell-platform-linux`.

## Related crates

- `tpt-shell`: the facade that most apps use.
- `tpt-shell-ipc`, `tpt-shell-capabilities`, `tpt-shell-manifest`: depend on this crate for shared types.
- `tpt-shell-replay`: uses `Clock` and `Rng` to record and replay runs.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
