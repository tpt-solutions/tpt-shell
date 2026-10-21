// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Test support for tpt-shell apps (spec §10.6).
//!
//! The test driver is not built yet. [`visual`] compares two screenshots on raw pixels, so it
//! needs no image decoder. [`a11y`] holds the accessibility rules that need only numbers and
//! names: contrast, target size and form labels.

pub mod a11y;
pub mod visual;
