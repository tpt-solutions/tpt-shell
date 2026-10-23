// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Core traits, geometry and error types for tpt-shell.
//!
//! Everything here is OS-independent. Platform crates implement the traits in
//! [`backend`]. The [`fake`] module implements them in memory for unit tests.
/// Support: Partial. Its traits are implemented by the Windows backend and the fake. See the items
/// in it.
pub mod backend;
/// Support: Full. Unit tested.
pub mod clock;
/// Support: Full. Unit tested.
pub mod error;
/// Support: Full. Unit tested.
pub mod executor;
/// Support: Full. In-memory test backend, used by the unit tests; it is not a platform.
pub mod fake;
/// Support: Full. Unit tested.
pub mod geometry;
/// Support: Full. Unit tested.
pub mod navigation;
/// Support: Partial. Clipboard, theme and dialogs are Windows verified. See the items in it.
pub mod platform;
/// Support: Full. Unit tested.
pub mod rng;
/// Support: Full. Unit tested.
pub mod theme;

pub use backend::{devtools_policy, PreferredScheme};
pub use clock::{Clock, FakeClock, SystemClock};
pub use error::{Error, Result, Support};
pub use executor::{block_on, Executor};
pub use geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
pub use platform::{
    ClipboardBackend, ColorScheme, MessageAnswer, MessageBox, MessageButtons, MessageIcon,
    OpenDialog, Platform, ThemeBackend,
};
pub use rng::{Rng, SeededRng, SystemRng};
pub use theme::{injection_script, Signals, Theme};
