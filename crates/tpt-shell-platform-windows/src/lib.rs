// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Windows backend for tpt-shell (spec §6.1).
//!
//! All `unsafe` code lives in this crate's platform modules (spec §9). On other OSes
//! this crate is empty.

#[cfg(windows)]
pub mod loader;
