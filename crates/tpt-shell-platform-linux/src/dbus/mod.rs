// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! D-Bus client and the services built on it (todo M5, decision 2).
//!
//! Every service here talks to the session bus through [`bus::session`]. That call fails with
//! `Unsupported` unless the `dbus` feature is on, so with the feature off the shell opens no
//! socket and every D-Bus service reports that it is unavailable.
//!
//! - [`wire`]: message encoding and decoding, no socket.
//! - [`bus`]: the connection and EXTERNAL authentication.
//! - [`notify`]: freedesktop notifications.
//! - [`secrets`]: the Secret Service keystore.
//! - [`settings`]: the colour scheme, through the desktop portal.
//! - [`tray`]: a StatusNotifier tray icon.
//! - [`atspi`]: a dump of an application's accessibility tree.

/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod atspi;
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod bus;
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod notify;
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod secrets;
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod settings;
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub mod tray;
/// Support: Full.
pub mod wire;

pub use bus::{session, Connection, Stream};
