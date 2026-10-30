# Changelog

All notable changes to `tpt-shell-manifest` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `parse` for `tpt-shell.toml`, returning a `Manifest` or a `ManifestError` with a `TS####` code and, where possible, the line.
- Unknown keys are rejected in the sections this phase owns.
- `[audit]` section: `store_params` and `redact`, validated against the published schema.
- Sections for later phases (`kiosk`, `devices`, `agent`, `replay`, `test`, `profiles`, `compliance`) are accepted and kept as raw TOML in `Manifest::stubs`.
- The `codes` module, with the error codes this crate returns.
