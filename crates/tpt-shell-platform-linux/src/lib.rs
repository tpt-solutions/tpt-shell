// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Linux backend for tpt-shell (spec §6.3).
//!
//! GTK and WebKitGTK are loaded with `dlopen` at run time ([`loader`]), so the shell builds
//! without their development packages and reports a missing library clearly.
//!
//! - [`dbus`]: a D-Bus client. Every D-Bus service sits behind the `dbus` cargo feature, which
//!   is off by default; with it off, the services return `Unsupported` and no socket opens.
//! - [`single_instance`]: one running copy, by an advisory lock file and a message drop.
//! - [`desktop`]: deep link registration through a desktop entry.
//! - [`hotkey`]: global hotkeys on X11 (Partial; Wayland is unsupported).
//! - [`probe`]: which GTK and WebKit libraries are installed, for `doctor`.
//! - [`gtk3`]: the GTK 3 and WebKit2GTK 4.1 adapter.
//! - [`gtk4`]: the GTK 4 and WebKitGTK 6.0 adapter.
//! - [`adapter`]: [`adapter::LinuxBackend`], which picks one of the two when it starts (GTK 4 if
//!   both load, `TPT_GTK=3|4` to force one).

/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod adapter;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod dbus;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod desktop;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod gtk3;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod gtk4;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod hotkey;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod loader;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod probe;
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub mod single_instance;

/// Helpers for the tests that start a real toolkit.
#[cfg(test)]
pub(crate) mod testutil {
    use std::sync::{Mutex, MutexGuard};

    /// The toolkit that started first in this test process. GTK 3 and GTK 4 must not both run in
    /// one process, and GTK is used from one test at a time, so a GUI test holds this lock for
    /// its whole run.
    static GUI: Mutex<Option<&'static str>> = Mutex::new(None);

    /// Takes the GUI lock for `toolkit`. `None` (after a message) when the other toolkit already
    /// started in this process: that test is skipped, and runs in the CI leg for its adapter.
    pub(crate) fn gui_session(
        toolkit: &'static str,
    ) -> Option<MutexGuard<'static, Option<&'static str>>> {
        let mut guard = GUI.lock().unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(started) if started != toolkit => {
                eprintln!("skipped: {started} already runs in this process, and {toolkit} cannot share it");
                None
            }
            _ => {
                *guard = Some(toolkit);
                Some(guard)
            }
        }
    }

    /// True when `TPT_LINUX_STRICT` is set (the CI adapter jobs): an environment that cannot run
    /// a test is then a failure, not a skip.
    pub(crate) fn strict() -> bool {
        std::env::var_os("TPT_LINUX_STRICT").is_some_and(|v| !v.is_empty())
    }

    /// Skips with a message, or fails when strict.
    pub(crate) fn skip_or_fail(why: &str) {
        assert!(!strict(), "{why} (TPT_LINUX_STRICT is set)");
        eprintln!("skipped: {why}");
    }
}
