# Changelog

All notable changes to `tpt-shell-profiles` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `Kind` (default, named and ephemeral) and `level` for each OS.
- `ProfileStore`: named profiles under one base folder, with checked names.
- `Ephemeral` sessions, wiped when dropped or closed.
- `wipe_dir`, a best-effort overwrite and delete.
- `usage` and `enforce_quota`, with a quota and a file-level isolation test.
- `export`: encrypted profile export with ChaCha20-Poly1305 and a PBKDF2-HMAC-SHA256 key (600,000 iterations).
- `switch` between profiles.
- The facade's `App::profile` picks the folder that windows keep their data in. An ephemeral session is wiped when the app stops.
