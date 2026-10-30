# tpt-shell-platform-linux

The Linux backend for tpt-shell. It drives GTK and WebKitGTK, which are loaded at run time with `dlopen`, so the shell builds without their development packages and reports a missing library clearly.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: written, not yet verified on a Linux desktop.** Both GTK adapters and the adapter selection type-check for x86_64 and aarch64 Linux. They run only in the CI adapter jobs (Xvfb and headless Weston). Wayland and X11 are not verified on real desktops. Not published (`publish = false`, version `0.0.0`).

## Adapters

| Adapter | Libraries | Module |
|---|---|---|
| GTK 3 | GTK 3.24 with WebKit2GTK 4.1 | `gtk3` |
| GTK 4 | GTK 4 with WebKitGTK 6.0 | `gtk4` |

`LinuxBackend` in `adapter` picks one when it starts. It uses GTK 4 when both load. Set `TPT_GTK=3` or `TPT_GTK=4` to force one.

`probe` reports which GTK and WebKit libraries are installed. `tpt-shell doctor` uses it.

## Modules

| Module | Contents |
|---|---|
| `loader` | Loads the libraries with `dlopen`. |
| `webkit_context` | The WebKit context shared by the windows. |
| `dbus` | A D-Bus client. Each service sits behind the `dbus` feature. |
| `single_instance` | One running copy, by an advisory lock file and a message drop. No socket is opened. |
| `desktop` | Deep-link registration through a desktop entry. |
| `hotkey` | Global hotkeys on X11 (Partial). Wayland is unsupported. |
| `probe` | Which libraries are installed. |

## Features

| Feature | Default | What it enables |
|---|---|---|
| `dbus` | off | D-Bus services: the StatusNotifier tray, notifications, Secret Service, the AT-SPI dump and the colour scheme. With the feature off, these calls return `Error::Unsupported`, and no socket opens. |

## Platform support

| Part | State |
|---|---|
| Window and WebKit adapters (GTK 3 and GTK 4) | Written. Run in CI under Xvfb and Weston. Not verified on a desktop. |
| Single instance | Written, uses a lock file. |
| Global hotkeys | Partial. X11 only. |
| D-Bus services (`dbus` feature) | Written, not verified on a desktop. |
| Keystore through Secret Service | Written behind `dbus`. |

## Usage

The backend is used through the facade. An app does not call this crate directly. To see what the machine has, run:

```sh
tpt-shell doctor
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
