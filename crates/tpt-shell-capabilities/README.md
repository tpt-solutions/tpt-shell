# tpt-shell-capabilities

The permission engine for tpt-shell. It decides whether a page may use a capability, and within which scope.

It is default deny. A request is allowed only when a grant names the capability, the window and the origin, and the request falls inside the grant's scope. Grants are per window and per origin, so one window cannot use another window's grants.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## Model

A **grant** names:

- a capability (for example `fs.read`),
- a window,
- an origin,
- a **scope** that limits the request, and
- optionally `consent: true`, which asks the user once per window and capability.

The engine returns a `Decision`. A refusal is a `Denial` that says which check failed.

## Scopes

| Scope | Matches |
|---|---|
| `Scope::Paths` | Directory roots. Paths are normalised first (`normalize_path`). |
| `Scope::PathGlobs` | Path patterns. `*` and `?` match within one segment. `**` matches any number of segments. Patterns are normalised, and a bad one fails when the grant is added. |
| `Scope::Devices` | Device identifiers, or globs over them. Each names a device kind. |
| `Scope::Urls` | URL prefixes, matched on a path-segment boundary. |
| `Scope::MaxBytes` | A size cap on the request. |

`real_path` resolves a path to its real location before a scope check. See the module documentation for how links are handled.

## Consent

A grant with `consent: true` asks the user through a `ConsentPrompt`. The answer is kept in a `ConsentStore`:

- `MemoryConsent` (the default) keeps answers for the life of the process.
- `FileConsent` keeps answers in a file, so a remembered answer survives a restart. The facade places this file in the window's data folder, so a named profile keeps it and an ephemeral session drops it.

The default answer to a consent prompt is No.

## Usage

```rust
use tpt_shell_capabilities::Engine;

let engine = Engine::new();
```

Grants are normally built by the facade (`App::grant`) from the manifest, not by hand.

## Platform support

None. The engine is platform-neutral. The Windows backend shows the consent dialog (`tpt-shell-platform-windows`).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
