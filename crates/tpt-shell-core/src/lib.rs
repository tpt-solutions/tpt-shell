// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Core traits, geometry and error types for tpt-shell.
//!
//! Everything here is OS-independent. Platform crates implement the traits in
//! [`backend`]. The [`fake`] module implements them in memory for unit tests.

pub mod backend;
pub mod error;
pub mod fake;
pub mod geometry;

pub use error::{Error, Result, Support};
pub use geometry::{LogicalSize, PhysicalSize, ScaleFactor};
