# tpt-shell-compliance

The compliance outputs for a tpt-shell app: software bills of materials (SBOMs), licence notices, a build attestation, detached signatures, and a licence gate that fails the build on a licence the policy does not allow.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## What it writes

| Output | Format |
|---|---|
| SBOM | CycloneDX and SPDX, each as JSON. |
| Notices | `licenses.html` and `THIRD_PARTY_NOTICES`. |
| Attestation | `attestation.json`, with the inputs and the reproducibility fields. |
| Signatures | Detached Ed25519 signatures over the outputs, with `--key`. |

The output is deterministic. Components are sorted by name and version. There is no timestamp unless the caller passes one.

## Inputs

- The JSON from `cargo metadata --format-version 1 --locked`.
- The lock file.
- `deny.toml`, from which the allow list is read at run time.

## The licence gate

The gate uses the same rule as cargo-deny's allow list:

- An `OR` expression passes when one of its branches is entirely on the allow list.
- An `AND` expression passes only when every licence in it is on the list.

The default allow list is `MIT`, `Unicode-3.0`, `BSD-3-Clause` and `ISC`. The project's `deny.toml` is the source of truth. The gate reads it, so the two cannot drift apart.

The gate is this crate's own parser. It is not cargo-deny itself. CI still runs cargo-deny.

## Usage

```rust
use tpt_shell_compliance::license_allowed;

assert!(license_allowed("MIT OR Apache-2.0"));
```

From the command line:

```sh
tpt-shell compliance <output-directory> --key <signing-key-file>
```

The command exits with status 1 when the gate refuses a licence. Error code `TS0122` is the gate's failure code. Run `tpt-shell explain TS0122` for the fix hint.

## Tests

One test checks that the shell's dependency graph contains no network crate. The shell opens no sockets by default, and this test keeps the dependency side of that promise honest.

## Platform support

None. The crate reads files and writes text. It runs on every platform.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
