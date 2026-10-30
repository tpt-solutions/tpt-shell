# Changelog

All notable changes to `tpt-shell-platform-linux` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- GTK 3.24 with WebKit2GTK 4.1, loaded at run time with `dlopen`.
- GTK 4 with WebKitGTK 6.0 adapter, and `LinuxBackend` selection (GTK 4 when both load, `TPT_GTK=3|4` to force one).
- Notifications, Secret Service keystore, AT-SPI dump and colour scheme, behind the `dbus` feature (off by default).
- X11 global hotkeys (Partial). Wayland is unsupported.
- Single instance by advisory lock file and message drop. No socket is opened.
- Deep-link registration through a desktop entry.
- `probe`, which reports installed GTK and WebKit libraries for `tpt-shell doctor`.

### Changed
- Single instance uses an advisory lock file and a message drop instead of a UNIX socket. No socket is opened.

### Notes
- Not yet run on a Linux desktop. Both adapters run only in the CI adapter jobs (Xvfb and headless Weston), which are non-blocking until green.
