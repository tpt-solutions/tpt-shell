# Changelog

All notable changes to `tpt-shell-cli` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `tpt-shell explain <TS####>` prints the registry entry for an error code.
- `tpt-shell manifest check` checks a manifest and reports each problem with its code and line.
- `tpt-shell new` creates vanilla and TypeScript starter projects, and Rust projects. `new --template` uses a template from `templates/`.
- `tpt-shell init` adds tpt-shell to an existing Rust crate.
- `tpt-shell add` writes `kiosk`, `devices`, `audit` or `agent` sections into the manifest, and refuses a section that already exists.
- `tpt-shell dev` serves an app's files on `127.0.0.1` and reports which files change. It prints the served folder without the Windows `\\?\` prefix.
- `tpt-shell build` (release build) and `tpt-shell run`.
- `tpt-shell doctor` reports what the build needs. It checks the licence allow list in `deny.toml`, and `--fix` installs `cargo-deny`.
- `tpt-shell contract diff` lists breaking and compatible changes. `contract check --baseline` exits 1 on a breaking change. `contract gen` writes the TypeScript client.
- `tpt-shell permissions suggest`, with `--audit` for the grants that refused calls were missing.
- `tpt-shell kiosk init` and `kiosk sign`.
- `tpt-shell audit verify | export | view | explain`. `verify` checks checkpoint signatures with `--key`, external anchors with `--anchors`, and rotated segments with `--rotated`.
- `tpt-shell replay` and `tpt-shell report` (bug-report bundle with `--redact`).
- `tpt-shell visual compare` writes an HTML report of screenshot differences.
- `tpt-shell compliance` writes SBOMs, the attestation, `licenses.html` and THIRD_PARTY_NOTICES. It signs outputs with `--key` and refuses on a licence the policy does not allow (exit 1).
- `tpt-shell runtime pin` and `runtime list`, for a pinned WebView2 runtime.
- `tpt-shell vendor` vendors dependencies.
- `tpt-shell install-service` and `uninstall-service`. On Windows this writes a per-user Run entry. On macOS and Linux it writes the LaunchAgent and systemd user units, and prints the commands without running them.
- Exit codes: 0 for success, 1 when the checked input has problems, and 2 for bad usage or an I/O failure.
