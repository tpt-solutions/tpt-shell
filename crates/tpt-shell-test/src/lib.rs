// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Test support for tpt-shell apps (spec §10.6).
//!
//! - [`driver`] and [`channel`]: the test driver. Built only with the `test-driver` feature.
//!   The control channel is a directory with a per-run token, not a socket or a named pipe,
//!   so it opens no network port (R7).
//! - [`visual`], [`screens`] and [`png`]: the screenshot comparer, the merge of screenshots
//!   from several CI jobs with its HTML report, and the PNG decoder they use.
//! - [`a11y`] and [`native`]: the accessibility rules, and the comparison of the DOM with the
//!   native accessibility tree.
//! - [`artifacts`]: the artifact collection that `tpt-shell test` writes.
//!
//! Support tags are given on each public item. The platform parts (the native tree dumps and
//! the screenshot capture) are routed to the platform crates.

pub mod a11y;
pub mod artifacts;
#[cfg(feature = "test-driver")]
pub mod channel;
#[cfg(feature = "test-driver")]
pub mod driver;
mod inflate;
pub mod native;
pub mod png;
pub mod screens;
pub mod visual;
