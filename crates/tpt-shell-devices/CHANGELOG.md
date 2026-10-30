# Changelog

All notable changes to `tpt-shell-devices` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Transport trait, with control transfers.
- `Hub`: opens a device only under a scope and audits opens, writes and closes.
- Virtual devices for tests: serial loopback, a printer with a receipt preview, a scanner and an HID device.
- Serial ports on Windows (`COM` ports, 8 data bits, no parity, one stop bit).
- TCP and UDP transports behind the `net-devices` feature, which is off by default.
- Receipt printing (ESC/POS) and label printing (ZPL).
- Raw spooler printing: through winspool on Windows, and through `lp` on macOS and Linux.
- Scanner support.
- ASCII-only text checks with `check_ascii`, which refuse characters the device cannot print.
- The facade re-exports this crate as `tpt_shell::devices`.
- A page command with a device grant can print through the hub.
