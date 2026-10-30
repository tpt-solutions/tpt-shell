# Changelog

All notable changes to `tpt-shell-platform-macos` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- AppKit window (frameless, transparent, always-on-top, vibrancy backdrop and close action), and WKWebView with the script message bridge.
- Document-start scripts, the navigation policy and the `app://` scheme handler.
- Snapshots, message boxes and file dialogs, pasteboard text and appearance observation.
- Menu bar tray and single instance by bundle identifier.
- Keychain through the Security framework.
- Runtime tests that run on macOS CI and skip without a GUI session.

Not yet run on a Mac. The event loop, notifications, deep links and accessibility are not built.
