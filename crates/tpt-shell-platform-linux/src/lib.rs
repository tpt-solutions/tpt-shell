// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Linux backend for tpt-shell (spec §6.3).
//!
//! GTK and WebKitGTK are loaded with `dlopen` at run time ([`loader`]), so the shell builds
//! without their development packages and reports a missing library clearly.
//!
//! - [`dbus`]: a D-Bus client. Every D-Bus service sits behind the `dbus` cargo feature, which
//!   is off by default; with it off, the services return `Unsupported` and no socket opens.
//! - [`single_instance`]: one running copy, by an advisory lock file and a message drop.
//! - [`desktop`]: deep link registration through a desktop entry.
//! - [`hotkey`]: global hotkeys on X11 (Partial; Wayland is unsupported).
//! - [`probe`]: which GTK and WebKit libraries are installed, for `doctor`.
//! - [`gtk3`]: the GTK 3 and WebKit2GTK 4.1 adapter.

pub mod dbus;
pub mod desktop;
pub mod gtk3;
pub mod hotkey;
pub mod loader;
pub mod probe;
pub mod single_instance;
