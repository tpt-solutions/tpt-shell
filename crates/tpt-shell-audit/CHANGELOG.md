# Changelog

All notable changes to `tpt-shell-audit` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Append-only log with a SHA-256 hash chain. One canonical JSON record per line, and `verify_file` reports the first bad sequence number.
- `Signing`: Ed25519-signed checkpoints after a number of entries or after a number of seconds (`every_secs`).
- `verify_file_with_key` checks checkpoint signatures with a public key.
- File anchors and syslog-style anchors, checked by `verify_file_anchored`.
- Size-based rotation with chain continuity across segments (`RotatingLog`, `verify_rotated`).
- Truncation detection when anchors are set.
- Keys: the checkpoint signer, and key supplier traits.
- `export_csv`, and the `tpt-shell audit verify | export | view` commands.
- `Policy`, which decides which arguments are recorded or hashed, and redacts the named fields.
- A seeded property test over signed chains with checkpoints. Byte flips, removed records and swapped records are all caught.
- Windows: the checkpoint key is kept in the DPAPI keystore.
