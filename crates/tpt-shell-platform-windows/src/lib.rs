// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Windows backend for tpt-shell (spec §6.1).
//!
//! All `unsafe` code lives in this crate's platform modules (spec §9). On other OSes
//! this crate is empty.

#[cfg(windows)]
pub mod accessibility;
#[cfg(windows)]
pub mod backend;
#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod com;
#[cfg(windows)]
pub mod consent;
#[cfg(windows)]
pub mod deep_link;
#[cfg(windows)]
pub mod dialogs;
#[cfg(windows)]
pub mod event_handler;
#[cfg(windows)]
pub mod handler;
#[cfg(windows)]
pub mod keystore;
#[cfg(windows)]
pub mod loader;
#[cfg(windows)]
pub mod local_time;
#[cfg(windows)]
pub mod random;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod shim;
#[cfg(windows)]
pub mod single_instance;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod webview2;
#[cfg(windows)]
pub mod window;
