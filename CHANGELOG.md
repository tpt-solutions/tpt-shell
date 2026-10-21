# Changelog

All notable changes to tpt-shell are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The project will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Repository skeleton: Cargo workspace, empty facade crate `tpt-shell`.
- Dual licence: MIT and Apache-2.0 (TPT Solutions).
- Licence and advisory policy in `deny.toml`.
- Governance: SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, this changelog.
- CI skeleton: fmt, clippy, test on three operating systems, cargo-deny, SPDX header check.
- `tpt-shell audit verify | export | view`: checks the hash chain (and checkpoint signatures with `--key`), exports verified logs as CSV or JSON Lines, and prints a short table.
- `[audit]` manifest section: `store_params` and `redact`, validated against the published schema.
- Command errors can carry structured `details`; invalid arguments report the offending path.
- MSRV is Rust 1.90, set in the workspace manifest and measured on 1.90.0.
- Windows: release builds turn off the developer tools and the right-click menu for each page window (debug builds keep them on).
- Windows: every page the shell serves carries a strict Content-Security-Policy (`script-src` and `style-src` limited to the app's own origin; inline script and style are blocked).
- `[[windows]] allow_navigation`: `https://` URL prefixes a window may navigate to. Remote navigation is blocked otherwise (Windows).
- Windows: a window can receive messages from its page (`set_message_handler`). The handler's reply is posted back to the page. A runtime test runs a capability-checked command through it.
- `tpt_shell::App` (Windows): `App::new().loader(...).window(Window::new("main").html(...)).grant(...).command(...).run()`. Pages call guarded commands through the checked router; consent is refused until the native dialog exists.
- IPC: commands can be registered behind a capability (`register_guarded`). `handle_authorized` checks the caller's window and origin grant before the handler runs, and asks for consent when a grant needs it. Guarded commands are refused on the unauthorized entry points.
