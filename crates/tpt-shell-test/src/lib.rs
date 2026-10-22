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

/// Support: Partial. The browser side that collects focus stops, Tab traces and animations is
/// routed to the platform agents. The rules are heuristics where marked.
pub mod a11y;
/// Support: Full.
pub mod artifacts;
/// Support: Full. Built only with the test-driver feature, which is off in normal builds.
#[cfg(feature = "test-driver")]
pub mod channel;
/// Support: Full. Built only with the test-driver feature, which is off in normal builds.
#[cfg(feature = "test-driver")]
pub mod driver;
mod inflate;
/// Support: Partial. The default native tree dump returns Unsupported. The Windows, macOS and Linux
/// overrides are routed to the platform agents.
pub mod native;
/// Support: Partial. Adam7 interlaced files are refused, and tRNS colour keys on grey and RGB
/// images are ignored.
pub mod png;
/// Support: Partial. The HTML report lists results and has no side-by-side or diff images yet.
pub mod screens;
/// Support: Full.
pub mod visual;
