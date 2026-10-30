# Changelog

All notable changes to `tpt-shell` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Facade crate with the `App`, `Window`, `Scope` and command types, and re-exports of the core, IPC, capability, audit, profile and macro crates.
- Windows: `App::new().loader(...).asset(...).window(...).grant(...).command(...).run()` opens a window, serves its pages from the app origin, and runs capability-checked commands. A grant that needs consent asks in a native dialog, and the default answer is No.
- Windows: `App::audit(path, policy)` writes each command call from a page to a hash-chained audit log. `App::manifest_audit(manifest, log)` does the same from the `[audit]` section of the manifest.
- Windows: `App::tray`, `App::single_instance`, `App::deep_link`, `App::remember_window_state`, the keystore and balloon notifications.
- Windows: `put_blob` and `remove_blob` for binary data a page can fetch, with a 64 MiB store limit.
- Windows: `Running::emit` sends an event to a window's page, which receives it through `tpt.on`.
- `App::profile(ProfileKind::Named(..) | Ephemeral)` picks the folder a window keeps its data in. An ephemeral session is wiped when the app stops.
- `App::kiosk_config` applies a signed kiosk config. `App::snapshot_state` saves and restores app state.
- `App::contract` and `App::write_contract`, and the `--tpt-write-contract <path>` flag.
- Features `agent`, `admin-endpoint`, `health-endpoint` and `devtools`. All are off by default.
- `app://__tpt/licenses` serves the licence page when one is embedded at build time.
- `examples/hello`, which opens one window with one embedded page.
- Release builds turn off the developer tools and the right-click menu on Windows. Debug builds keep them on.
- Every page the shell serves on Windows carries a strict Content-Security-Policy.

### Changed
- MSRV is Rust 1.90, set in the workspace manifest.
