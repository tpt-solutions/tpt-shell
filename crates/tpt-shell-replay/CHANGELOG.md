# Changelog

All notable changes to `tpt-shell-replay` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `.tptreplay` format as JSON Lines, with a versioned header.
- Format version 2, with assets (SHA-256 checked on read) and clock and random-number event kinds. Version 1 recordings still read.
- `Mode::Strict` and `Mode::Lenient` readers.
- `Recorder`, with plain-text and zstd output (`finish_compressed` and `read_compressed`).
- `divergence`, which compares a recording with a new run's events, ignoring timing.
- `capture` helpers, one per event kind.
- `virtual_time`, which makes the clock and random-number traits replayable, and an in-page shim for time and randomness.
- `player`, which runs a recording against a target.
- `timeline`, the data model for time-travel.
- `report`, a bug-report bundle with redaction.
- `tpt-shell replay` plays a recording against the echo reference target. `tpt-shell report` writes a bug-report bundle.
- Size limits: 256 MiB per recording and 16 MiB per asset.
