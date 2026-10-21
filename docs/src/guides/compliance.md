# Compliance outputs

This guide covers the files that `tpt-shell compliance` writes for a release: software bills of
materials (SBOMs), an attestation, and the licence gate. It is for people who answer procurement and
security questionnaires, and for the developers who run the release.

Status: the two SBOM formats and the attestation are written and validated against the published
schemas. The licence gate works. Licence notices (`licenses.html`), `THIRD_PARTY_NOTICES`, detached
signatures, the runtime pin and the bundling and signing steps are not built (todo M10).

## Running it

From the root of the app's Rust project, with the lock file present:

```sh
tpt-shell compliance out/compliance
```

The command runs `cargo metadata --format-version 1 --locked`, so the lock file must match
`Cargo.toml`. It writes three files into the output directory:

| File | What it is |
|---|---|
| `sbom.cdx.json` | CycloneDX 1.5 SBOM |
| `sbom.spdx.json` | SPDX 2.3 SBOM |
| `attestation.json` | What was built, with digests of the other two files |

The command prints the number of components written. It exits with 1 when the licence check finds a
violation, and the attestation still records it.

## Determinism

The output is sorted by component name and version. There is no timestamp unless you set
`SOURCE_DATE_EPOCH`, which is then recorded in the attestation. The attestation records the rustc
version, the digest of `Cargo.lock`, the git commit when one is available, and the build flags
(`--locked`).

## Licence gate

Two checks use the licence rules, and they are not the same code:

- `tpt-shell compliance` checks every component against the crate's built-in allow list, and records
  each violation in the attestation. It does not read `deny.toml`.
- The licence check in the CLI reads the allow list from `deny.toml` in the current directory. It
  reports `TS0120` when that file cannot be read, and `TS0122` when a dependency's licence is not on
  the list. Its parser is this crate's own, not cargo-deny.

The rule is the same as cargo-deny's: an expression passes when one of its OR branches is entirely on
the list, and an AND expression needs every licence on it. CI still runs `cargo deny check`, which
is the authority. Do not let the gate replace it.

Every dependency must declare a licence. A component with none is a violation.

## Checking the SBOMs

The schemas are checked by `tools/validate-sbom.sh`, which runs both files against the published
CycloneDX 1.5 and SPDX 2.3 schemas. That script is not yet a CI job (todo M10).

## Reproducible builds

The compliance outputs record the settings that matter for a reproducible build: `--locked`, and
`SOURCE_DATE_EPOCH` when set. Path remapping and a rebuild test that compares two builds of the same
commit are not done. Until they are, say that builds are locked, not that they are reproducible.

## Runtime and offline use

The WebView2 runtime is a separate component. The pinned Fixed Version runtime and the air-gap
guidance (`tpt-shell runtime pin`, `tpt-shell vendor`, `tpt-shell docs --offline`) are not built.
The shell opens no sockets by default, and a test checks that.

## What a procurement pack needs

A pack usually asks for a questionnaire, a data-flow diagram, the SBOM, a licence summary and an
explanation of audit and kiosk behaviour. The SBOMs and attestation are built. The rest is written by
hand, and the template is not done (todo M10). Link to the [Security guide](../security.md) and the
[Audit guide](audit.md) for the behaviour, and say which items on those pages are not built.

## Status

| Item | Status |
|---|---|
| CycloneDX 1.5 SBOM | Built, validated against the schema by script |
| SPDX 2.3 SBOM | Built, validated against the schema by script |
| Attestation | Built |
| Licence gate with the built-in allow list | Built, unit-tested |
| Licence gate from `deny.toml` (CLI check) | Built |
| Licence notices and embedding | Not built |
| `THIRD_PARTY_NOTICES` generation | Not built |
| Detached signatures on outputs | Not built |
| Bad-dependency test in the build | The gate is unit-tested; nothing runs it from the build yet |
| Reproducibility test | Not built |
| Procurement pack template | Not built |
