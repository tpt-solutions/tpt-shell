# Changelog

All notable changes to `tpt-shell-kiosk` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Kiosk config that applies only with a valid Ed25519 signature.
- Single-app policy, navigation allow-list and placement values for the backends to apply.
- Atomic state snapshots, saved on an interval and at exit, restored at the next start.
- Idle timer that resets the kiosk to its home page.
- Crash-recovery decisions: a crashed page reloads the last good page.
- Blocked key chords (Alt+Tab, Win, Alt+F4, Ctrl+Esc) and a Windows low-level keyboard hook for them, best effort.
- Heartbeat file for the supervisor.
- `admin-endpoint` and `health-endpoint` features: loopback only, token protected for admin, off by default.
- `App::kiosk_config` and `App::snapshot_state` in the facade. `tpt-shell kiosk init` and `tpt-shell kiosk sign` in the command line tool.
