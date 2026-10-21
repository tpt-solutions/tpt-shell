# Contributing to tpt-shell

Thanks for helping. By contributing you agree that your work is licensed under MIT OR Apache-2.0, with no additional terms (see the Contribution section of [README.md](README.md)).

## Before you start

- Read [spec.txt](spec.txt) for the design and [todo.md](todo.md) for what is planned and in progress.
- For a change to the design, the public API or the contract, open an RFC first (see [docs/rfcs/](docs/rfcs/)).
- Security issues go through [SECURITY.md](SECURITY.md), not public issues.

## Dependency rules

- New dependencies must be MIT, or dual-licensed with MIT.
- Apache-2.0-only crates are rejected. GPL, LGPL, MPL and AGPL are rejected by default.
- Permissive extras (BSD-3-Clause, Unicode-3.0, Unlicense, ISC, Zlib) need a one-line justification in `deny.toml`.
- Keep the list of crates short. Prefer the standard library.
- `cargo deny check` must pass.

## Code rules

- Every source file starts with the SPDX header:
  - Rust, TOML, shell and YAML: `# SPDX-License-Identifier: MIT OR Apache-2.0` (or `//` for Rust)
  - Then `Copyright (c) 2026 TPT Solutions`
- `unsafe` and FFI live only in platform modules, each with its own tests.
- No telemetry and no network access unless a feature is enabled.
- Every public API item carries a support tag: Full, Partial (with the gap stated), or Unsupported.
- Every error has a stable `TS####` code, an explanation and a fix hint.
- Do not claim "first" or "only" in docs without evidence in `docs/competitive-scan.md`.

## Checks to run before opening a PR

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
```

## Versioning and MSRV

- The project follows Semantic Versioning once v1.0.0 is released. Before then, minor versions may break compatibility.
- A breaking change to the contract (typed IPC) must pass `tpt-shell contract diff` once that command exists.
- The minimum supported Rust version (MSRV) is **1.90**. It is set once in `[workspace.package]` (`rust-version`) and inherited by every crate, so Cargo refuses to build on an older toolchain.
- The MSRV was measured, not assumed: the whole workspace builds and passes its tests on Rust 1.90.0. Older installed toolchains fail, either on a dependency that needs a newer Rust or on `unsigned_is_multiple_of` (stable in 1.87).
- Raising the MSRV is a minor-version change at the least. It is recorded in CHANGELOG under "Unreleased" with the reason, and it happens only when a dependency or a language or library feature needs it.
- Deprecations are announced in the CHANGELOG for at least one minor release before removal, once releases exist.

## Pull requests

- Use the PR template. Keep a PR to one logical change.
- Update [CHANGELOG.md](CHANGELOG.md) under "Unreleased" for user-visible changes.
- Tick the matching item in [todo.md](todo.md) when a task is finished.

## Support tags and conformance

- Every public API item states its support in its doc comment with a line that starts `Support:` and names `Full`, `Partial` (say what is missing) or `Unsupported` (returns `Error::Unsupported`). The docs build renders these tags from the source.
- The conformance suite lives in `conformance/`, which is its own Cargo workspace. Run it with `cargo test --manifest-path conformance/Cargo.toml`. Checks that need a platform backend that is not built yet report as pending and do not fail. Set `TPT_CONFORMANCE_REQUIRE_PASS=1` to make pending checks fail.
