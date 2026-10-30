# Changelog

All notable changes to `tpt-shell-capabilities` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Default-deny `Engine` with per-window, per-origin grants.
- Scopes: `Paths`, `PathGlobs`, `Devices`, `Urls` and `MaxBytes`.
- `PathGlobs`: `*` and `?` match within one segment, and `**` matches any depth. Patterns are normalised, and a bad one fails when the grant is added.
- `ConsentPrompt` and `ConsentStore`, with an in-memory store (`MemoryConsent`).
- `FileConsent`, which keeps consent answers in a file so a remembered answer survives a restart.
- `normalize_path` and `real_path`.
