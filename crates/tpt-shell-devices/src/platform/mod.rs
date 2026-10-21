// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Operating-system device code. Everything with `unsafe` or FFI lives in these modules, each
//! behind a `cfg` for its platform, so other platforms still build.

#[cfg(unix)]
pub mod unix_spool;
#[cfg(windows)]
pub mod win_serial;
#[cfg(windows)]
pub mod win_spool;
