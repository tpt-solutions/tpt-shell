// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Test support for tpt-shell apps (spec §10.6).
//!
//! The test driver and the accessibility audit are not built yet. [`visual`] compares two
//! screenshots, and works on raw pixels so it needs no image decoder.

pub mod visual;
