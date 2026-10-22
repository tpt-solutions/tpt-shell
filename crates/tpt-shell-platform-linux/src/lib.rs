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

/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod dbus;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod desktop;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod gtk3;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod hotkey;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod loader;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod probe;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod single_instance;
