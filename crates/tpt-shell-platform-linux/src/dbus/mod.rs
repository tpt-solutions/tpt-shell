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

pub mod atspi;
pub mod bus;
pub mod notify;
pub mod secrets;
pub mod settings;
pub mod tray;
pub mod wire;

pub use bus::{session, Connection, Stream};
