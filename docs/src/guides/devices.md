# Devices

This guide covers serial ports, HID devices, USB devices, printers, scanners, scales and network
devices. It is for developers who open devices from Rust, and for operators who decide which
devices an app may use.

Status: the identifiers, scopes, hub, audit, virtual devices and the receipt and label builders
are built and tested. Hardware access differs by platform, so read the table at the end before you
promise a device to a customer. The page-facing capabilities (`device.serial` and the others) are
not wired to the page yet. Use the hub from Rust code.

## Identifiers

A device has a canonical text form. It is the same string in logs, in the manifest and in code:

- `serial:COM3` (port names compare without case)
- `hid:1234:5678` and `usb:1234:5678` (vendor and product, four hex digits each)
- `net:printer.local:9100` (host and port)

Parse one with `FromStr`. A bad identifier is refused with a parse error.

## Scopes

A `DeviceScope` says which devices a caller may open. Each scope matches one kind only, so a serial
scope never matches a network device.

- `Serial { port }`: port names, with `*` and `?` globs, for example `COM*`.
- `Hid { vendor, product }` and `Usb { vendor, product }`: `None` means any value for that field.
- `Net { host, port }`: host name with globs, and a port or any.

Keep scopes narrow. A scope of `COM*` lets the app open every serial port on the machine.

## The hub

`DeviceHub` is built from the devices that are present and an audit callback. It is the only way
the app opens a device under a scope:

- `list(&scope)` returns the identifiers the scope covers.
- `open(&id, &scope)` opens a device. It refuses an id outside the scope, a device that is not
  present, and a device that is already open. Each device opens once.
- `open_with(&id, &scope, &options)` takes `OpenOptions` (serial line settings, the USB interface,
  a transfer timeout). A device the hub was built with is used as it is. Otherwise the real device
  is opened through the platform, scope first. `open` keeps its old behaviour: built-in devices only.
- `discover(&scope)` adds the real devices the platform lists to the hub's own.
- An open device writes with `write`, reads with `read`, runs `control` (USB control transfers,
  Linux raw USB only; other devices refuse) and closes when it is dropped.

Every open, write, control transfer and close is passed to the audit callback. A write records the number of bytes
taken. Reads are not audited. Payloads are not logged.

## Printing and labels

The builders lay out bytes; a transport sends them:

- `escpos::Receipt` builds ESC/POS receipt bytes: alignment, bold, lines, feeds and cuts.
- `zpl` builds Zebra label commands, with text and Code 128 barcodes.
- `jobs::receipt` and `jobs::label` lay out a common receipt or label.

Text is ASCII. Non-ASCII text is refused with an error rather than printed in the wrong code page.

Raw printing through the operating system's spooler is a separate path: on Windows it uses the
winspool RAW datatype. On macOS and Linux it calls CUPS `lp -o raw`. That needs CUPS on the
machine, and it has not been run from this workspace on those systems.

## Virtual devices

Use virtual devices in tests and demos. They need no hardware:

- `VirtualSerial` is a loopback port with a capacity limit.
- `VirtualPrinter` keeps the bytes it receives, and `preview()` shows the receipt as text.
- `VirtualScanner` sends scripted scans, each followed by a carriage return.
- `VirtualHid` takes input and output reports through a handle.

`virtual_devices::from_spec` builds a set from a spec such as `printer@usb:0416:5011;scanner@hid:0c2e:0b61`.
The environment variable `TPT_VIRTUAL_DEVICES` is meant to carry that spec, but the app does not
read it yet.

## Network devices

Raw TCP and UDP to network printers and devices sit behind the `net-devices` feature. It is off by
default, so a default build opens no socket. Network name lookup uses the system resolver, which can
block for as long as the system takes to answer. A timeout during a write that has already sent
some bytes is reported as an error, not as busy, because those bytes cannot be taken back.

## Scanners and scales

The scanner module parses the text a keyboard-wedge scanner types, and the serial and HID POS paths share its parser. The HID POS path reads
only boot-protocol keyboard reports, so a scanner that needs its own report descriptor is not
handled. The scale module parses a common line shape. A scale that sends a different shape needs its
own parser.

## Hot-plug

`hotplug::Watch` is off by default and sees nothing. An opted-in watcher (`Watch::opted_in()`)
compares device lists, and `poll_native(&scope)` builds the list from the platform: `/dev` serial
nodes on macOS and Linux, hidraw and usbfs on Linux, IOKit HID on macOS. This is polling, not an
operating-system event stream, and Windows has no listing yet.

## Linux permissions (udev)

hidraw and usbfs nodes are owned by root, so an ordinary user is refused ("a udev rule must grant
access"). Grant access to the devices an app needs, by vendor and product, never to every device.
Put a file such as `/etc/udev/rules.d/70-tpt-devices.rules` on the machine:

```
# HID device 1a2b:0001 (hidraw)
KERNEL=="hidraw*", SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1a2b", ATTRS{idProduct}=="0001", MODE="0660", GROUP="plugdev", TAG+="uaccess"
# Raw USB device 0403:6001 (usbfs)
SUBSYSTEM=="usb", ATTR{idVendor}=="0403", ATTR{idProduct}=="6001", MODE="0660", GROUP="plugdev", TAG+="uaccess"
```

Then run `udevadm control --reload-rules && udevadm trigger` and replug the device. Serial ports
(`/dev/ttyUSB*`, `/dev/ttyACM*`) belong to the `dialout` group on most distributions (`uucp` on
some), so add the user to it. Raw USB cannot claim an interface that a kernel driver owns, so
bulk transfers are refused there, although control transfers still work. On macOS, reading a
keyboard-class HID device needs the Input Monitoring permission.

## Operator checklist

- Grant the narrowest scope the app needs.
- Keep the audit log on. Check that opens and writes appear in it.
- Use the device's own id in the scope, not a glob, where you can.
- Test on the real device before you ship. The virtual devices check the code, not the hardware.

## Status by platform

| Device path | Windows | macOS | Linux |
|---|---|---|---|
| Serial (`serial:COMn`, 8 data bits, no parity, 1 stop bit) | Built and tested | Built (termios, all line settings); type-checked, not run on a Mac | Built (termios, all line settings); run on CI only if a port exists |
| Serial port discovery | Not built; the caller names the port | `/dev/cu.*` listing; type-checked | `/dev/ttyUSB*`, `ttyACM*`, `ttyS*`; run on CI |
| HID | Not built | IOKit `IOHIDManager`; type-checked only, never run | hidraw; run on CI only if a device exists |
| USB raw (bulk, control) | Not built (WinUSB) | Not built (IOKit USB) | usbfs; run on CI only if a device exists |
| Raw print through the spooler | Built (winspool RAW) | Partial (CUPS `lp`, not run from here) | Partial (CUPS `lp`, not run from here) |
| Network TCP and UDP (feature `net-devices`) | Built | Built, not run on macOS | Built, not run on Linux |
| Virtual devices | Built and tested | Built; round-trip tests not run on macOS | Built; round-trip tests not run on Linux |

The real-device matrix (one serial adapter, one HID scanner, one ESC/POS printer) is not done
(todo M7). Do not claim support for a device that is not in that matrix.
