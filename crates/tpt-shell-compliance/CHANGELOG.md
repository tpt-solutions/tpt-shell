# Changelog

All notable changes to `tpt-shell-compliance` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- CycloneDX and SPDX SBOM generation from `cargo metadata` output.
- `licenses.html` and `THIRD_PARTY_NOTICES` generators.
- `attestation.json`, with the reproducibility fields.
- Detached Ed25519 signatures for the outputs.
- The licence gate, which reads the allow list from `deny.toml` and refuses a licence the policy does not allow (error code `TS0122`).
- A test that the shell's dependency graph contains no network crate.
- `tpt-shell compliance` writes the outputs, signs them with `--key`, and exits 1 on a refused licence.
