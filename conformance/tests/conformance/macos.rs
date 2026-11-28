// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The macOS checks. The checks are shared (`shared.rs`); this file gives the WKWebView backend its
//! constructor, its main-loop pump and the run.
//!
//! The backend needs the process main thread, which the suite's `main` runs on. Without a GUI
//! session the checks cannot run, and they are reported as skipped.

use std::time::Duration;

use tpt_shell_conformance::{Outcome, RunReport};
use tpt_shell_core::theme::Theme;
use tpt_shell_platform_macos::backend::MacBackend;

use super::shared::{self, Host, Step};

const BACKEND: &str = "wkwebview";
/// The origin the macOS backend serves app pages from (see `policy::APP_ORIGIN`).
const APP_ORIGIN: &str = "app://localhost";

impl Host for MacBackend {
    const ORIGIN: &'static str = APP_ORIGIN;

    fn open() -> Step<Self> {
        MacBackend::new().ok_or_else(|| {
            Outcome::Skipped("the backend must be created on the main thread".into())
        })
    }

    fn pump(&mut self, done: &mut dyn FnMut() -> bool, timeout: Duration) -> bool {
        self.run_until(done, timeout)
    }

    fn set_theme(&mut self, theme: Theme) -> tpt_shell_core::Result<()> {
        MacBackend::set_theme(self, theme)
    }
}

/// Runs every check on the macOS backend and returns the report for this OS.
pub fn run() -> RunReport {
    shared::run_checks::<MacBackend>(std::env::consts::OS, BACKEND)
}
