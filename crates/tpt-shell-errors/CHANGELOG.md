# Changelog

All notable changes to `tpt-shell-errors` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `REGISTRY` of stable `TS####` codes, each with a title, an explanation and a fix hint.
- `lookup(code)`, which ignores case and surrounding whitespace.
- Codes `TS0100` to `TS0122` (audit, agent and the licence gate) and `TS0301` to `TS0305` (the test driver).
- `tpt-shell explain <TS####>` uses this registry.
