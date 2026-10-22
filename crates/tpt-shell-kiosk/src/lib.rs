// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Kiosk support for tpt-shell (spec §10.1, M6). Parts that work without a window:
//!
//! - [`config`]: a kiosk config that only applies when its Ed25519 signature verifies.
//! - [`policy`]: the values a signed config sets (single-app mode, navigation allowlist,
//!   full-screen and single-window placement, and the feature-disable list). Backends apply them.
//! - [`state`]: a state snapshot saved on an interval and at exit, restored at the next start.
//! - [`idle`]: the idle timer that sends a kiosk back to its home page.
//! - [`recovery`]: page crash recovery, which reloads the last good page.
//! - [`chords`]: the key chords a kiosk blocks (Alt+Tab, Win, Alt+F4, Ctrl+Esc).
//! - [`heartbeat`]: the heartbeat file the supervisor watches. Written only when supervised.
//! - [`admin`] and [`health`]: local HTTP endpoints. Each needs its feature (`admin-endpoint`,
//!   `health-endpoint`), and both are off by default, so a default build opens no socket.
//!
//! The Windows low-level keyboard hook is in `hook_windows` (Windows only). It is best effort.
//!
//! # OS lock-down recipes
//!
//! A kiosk app cannot lock the whole desktop from user space. Use the OS to do that, and let the
//! app handle what it can:
//!
//! - Windows: Assigned Access (kiosk mode) in Settings, or `Set-AssignedAccess` with the user
//!   name and the app's package family name, on Windows Pro or Enterprise. Ctrl+Alt+Del and the
//!   power menu stay available, so disable them by policy if the device needs it.
//! - macOS: Guided Access for a single session, or an MDM profile that sets the Dock, the Login
//!   Items and the allowed apps. A LaunchAgent (written by the supervisor's install code) starts
//!   the supervisor at login.
//! - Linux: a dedicated session that starts only the app, for example an autologin account whose
//!   session runs the supervisor and the app as a systemd user unit, with the window manager's
//!   own key bindings removed.
//!
//! The key-chord hook blocks only the chords it knows about. Ctrl+Alt+Del is handled by Windows
//! itself and cannot be blocked from user space.

/// Support: Full.
pub mod chords;
/// Support: Full.
pub mod config;
/// Support: Full.
pub mod heartbeat;
/// Support: Full.
pub mod idle;
/// Support: Full.
pub mod policy;
/// Support: Full.
pub mod recovery;
/// Support: Full.
pub mod state;

/// Support: Partial. Windows only. Its own test installs and removes the hook; see hook_windows.
#[cfg(windows)]
pub mod hook_windows;

/// Support: Full. Built only with the admin-endpoint feature, which is off by default.
#[cfg(feature = "admin-endpoint")]
pub mod admin;
/// Support: Full. Built only with the health-endpoint feature, which is off by default.
#[cfg(feature = "health-endpoint")]
pub mod health;
/// Support: Full. Built only when an endpoint feature is on, and both are off by default.
#[cfg(any(feature = "admin-endpoint", feature = "health-endpoint"))]
pub mod http;
