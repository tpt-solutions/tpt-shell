# tpt-shell-devices

The device bridge for tpt-shell: serial ports, HID, USB and network transports, a hub that opens devices only under a scope and audits their use, virtual devices for tests, and builders for receipt printers and label printers.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). Serial ports and raw printing are built on Windows. Other transports and platforms are partial or not built.

## Access model

A page cannot open a device directly. An app grants device access with a scope, and the hub opens a device only when the scope allows it. Each open, write and close is audited.

Network devices use raw TCP and UDP sockets. They are behind the `net-devices` feature, which is off by default, so no socket opens unless it is enabled.

## Modules

| Module | Contents |
|---|---|
| `id`, `descriptors` | Device identifiers and what a device reports about itself. |
| `transport` | The transport trait. Supports control transfers. |
| `hub` | `Hub` opens devices under a `DeviceScope` and audits each use. `list` and `discover` show what the scope allows. |
| `serial` | Serial ports. |
| `net` | TCP and UDP transports (feature `net-devices`). |
| `native` | Platform-specific transports. |
| `hotplug`, `scanner`, `display` | Device arrival, scanners and display devices. |
| `virtual_devices` | Serial loopback, a printer with a receipt preview, a scanner and an HID device, for tests. |
| `escpos` | ESC/POS receipt printer commands. |
| `zpl` | ZPL label printer commands. |
| `spool` | Raw spooler printing. |
| `jobs`, `options`, `scale` | Print jobs, their options and scaling. |

## Text

Device text is ASCII. No code page is chosen for a device, so non-ASCII text is refused rather than printed as the wrong glyphs. `check_ascii` reports the first character that is not allowed.

## Usage

```rust
use tpt_shell_devices::check_ascii;

assert!(check_ascii("TOTAL 12.50").is_ok());
assert!(check_ascii("Café").is_err());
```

A page reaches a device through a command with a device grant. In an app, the facade's `devices` module re-exports this crate.

## Features

| Feature | Default | What it enables |
|---|---|---|
| `net-devices` | off | Raw TCP and UDP sockets to network devices. |

## Platform support

| Transport or feature | Windows | macOS | Linux |
|---|---|---|---|
| Serial ports (`COM1` to `COM999` on Windows, 8 data bits, no parity, one stop bit) | Built and tested on Windows | Written, not run | Written, not run |
| Raw printing | Windows spooler (winspool RAW), built | `lp` | `lp` |
| TCP and UDP (`net-devices`) | Built | Built | Built |
| HID and USB raw access | Not built | Not built | Not built |

Check `docs/src/platform-notes.md` for the current state before you rely on a transport.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option. The `libc` crate, used on Unix for the serial and printer transports, is MIT or Apache-2.0.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
