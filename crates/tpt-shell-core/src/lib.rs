// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Core traits, geometry and error types for tpt-shell.
//!
//! Everything here is OS-independent. Platform crates implement the traits in
//! [`backend`]. The [`fake`] module implements them in memory for unit tests.

pub mod backend;
pub mod clock;
pub mod error;
pub mod executor;
pub mod fake;
pub mod geometry;
pub mod platform;
pub mod rng;
pub mod theme;

pub use clock::{Clock, FakeClock, SystemClock};
pub use error::{Error, Result, Support};
pub use executor::{block_on, Executor};
pub use geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
pub use platform::{ClipboardBackend, ColorScheme, ThemeBackend};
pub use rng::{Rng, SeededRng, SystemRng};
pub use theme::{injection_script, Signals, Theme};
