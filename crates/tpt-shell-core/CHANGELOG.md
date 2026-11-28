# Changelog

All notable changes to `tpt-shell-core` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Backend traits in `backend`, with `devtools_policy` and `PreferredScheme`.
- `Clock` with `SystemClock` and `FakeClock`, and `Rng` with `SystemRng` and `SeededRng`.
- `Executor` and `block_on`.
- Geometry types: `LogicalSize`, `LogicalRect`, `PhysicalSize` and `ScaleFactor`.
- `Theme`, `Signals` and `injection_script` for colour scheme and accessibility settings.
- `Error`, `Result` and `Support`, the error type and support tag used across the workspace.
- `fake` module: an in-memory backend for unit tests.
- `Rgb` (an opaque colour), `Platform::accent_color` (the system accent colour), and the tray and menu types `TrayItem`, `Menu`, `MenuEntry` and `TRAY_ACTIVATE`.
- The fake backend keeps a tray icon, a menu bar and an accent colour. `FakeBackend::pick_menu` simulates a menu pick.
