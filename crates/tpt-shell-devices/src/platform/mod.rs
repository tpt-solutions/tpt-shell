// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Operating-system device code. Everything with `unsafe` or FFI lives in these modules, each
//! behind a `cfg` for its platform, so other platforms still build.

#[cfg(unix)]
/// Support: Partial. Goes through CUPS lp with -o raw, which needs CUPS installed; not run on
/// macOS/Linux here; compile-checked.
pub mod unix_spool;
#[cfg(windows)]
/// Support: Full.
pub mod win_serial;
#[cfg(windows)]
/// Support: Full.
pub mod win_spool;
