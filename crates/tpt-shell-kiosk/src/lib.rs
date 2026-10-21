// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Kiosk support for tpt-shell (spec §10.1, M6). Three parts, each usable without a window:
//!
//! - [`config`]: a kiosk config that only applies when its Ed25519 signature verifies.
//! - [`state`]: a state snapshot saved on an interval and at exit, restored at the next start.
//! - [`idle`]: the idle timer that sends a kiosk back to its home page.

pub mod config;
pub mod idle;
pub mod state;
