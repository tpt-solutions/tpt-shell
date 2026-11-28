// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The Linux checks. The checks are shared (`shared.rs`); this file gives the GTK backend its
//! constructor, its main-loop pump and the run.
//!
//! Needs a display (`DISPLAY` or `WAYLAND_DISPLAY`) and GTK 3 or 4 with WebKitGTK at run time.
//! Without them every check is reported as skipped, or as failed when `TPT_LINUX_STRICT` is set.

use std::time::Duration;

use tpt_shell_conformance::{Outcome, RunReport};
use tpt_shell_core::theme::Theme;
use tpt_shell_platform_linux::adapter::LinuxBackend;

use super::shared::{self, Host, Step};

const BACKEND: &str = "webkitgtk";
/// The origin the Linux backend serves app pages from (see `gtk3::route`).
const APP_ORIGIN: &str = "app://tpt-app";

fn strict() -> bool {
    std::env::var_os("TPT_LINUX_STRICT").is_some_and(|v| !v.is_empty())
}

impl Host for LinuxBackend {
    const ORIGIN: &'static str = APP_ORIGIN;

    /// A fresh backend. Without a display or the toolkit libraries the check is skipped, or failed
    /// under `TPT_LINUX_STRICT` (the CI job sets it, so a missing toolkit cannot pass silently).
    fn open() -> Step<Self> {
        LinuxBackend::new().map_err(|e| {
            if strict() {
                Outcome::Fail(e.to_string())
            } else {
                Outcome::Skipped(e.to_string())
            }
        })
    }

    fn pump(&mut self, done: &mut dyn FnMut() -> bool, timeout: Duration) -> bool {
        self.run_until(done, timeout)
    }

    fn set_theme(&mut self, theme: Theme) -> tpt_shell_core::Result<()> {
        LinuxBackend::set_theme(self, theme)
    }
}

/// Runs every check on the Linux backend and returns the report for this OS.
pub fn run() -> RunReport {
    shared::run_checks::<LinuxBackend>(std::env::consts::OS, BACKEND)
}
