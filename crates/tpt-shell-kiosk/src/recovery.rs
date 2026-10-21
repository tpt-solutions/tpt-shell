// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Page crash recovery (spec §10.1). When a page's renderer fails, the window reloads the last
//! page that loaded cleanly. If the page keeps failing, the window goes back to the home page
//! instead of reloading again. The decision is a pure function of the failures and their times.

use std::time::{Duration, Instant};

/// What the window should do after a page failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// Load this URL (the last good page, or home when there is none yet).
    Reload(String),
    /// Too many failures in the window. Load the home page and reset.
    Home,
}

#[derive(Clone, Debug)]
pub struct CrashRecovery {
    home: String,
    last_good: Option<String>,
    failures: Vec<Instant>,
    window: Duration,
    max_failures: u32,
}

impl CrashRecovery {
    /// `home` is the full URL of the home page. More than `max_failures` in `window` sends the
    /// window home.
    pub fn new(home: impl Into<String>, window: Duration, max_failures: u32) -> Self {
        CrashRecovery {
            home: home.into(),
            last_good: None,
            failures: Vec::new(),
            window,
            max_failures,
        }
    }

    /// Records that `url` finished loading without a failure.
    pub fn page_loaded(&mut self, url: &str) {
        self.last_good = Some(url.to_string());
    }

    /// Records a renderer failure at `now` and returns what to load.
    pub fn page_failed(&mut self, now: Instant) -> Recovery {
        let window = self.window;
        self.failures
            .retain(|t| now.saturating_duration_since(*t) < window);
        self.failures.push(now);
        if self.failures.len() as u32 > self.max_failures {
            self.failures.clear();
            self.last_good = None;
            return Recovery::Home;
        }
        Recovery::Reload(self.last_good.clone().unwrap_or_else(|| self.home.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_reloads_the_last_good_page() {
        let start = Instant::now();
        let mut recovery = CrashRecovery::new("app://home", Duration::from_secs(60), 2);
        assert_eq!(
            recovery.page_failed(start),
            Recovery::Reload("app://home".into()),
            "no good page yet, so home"
        );
        recovery.page_loaded("app://form?step=2");
        assert_eq!(
            recovery.page_failed(start + Duration::from_secs(5)),
            Recovery::Reload("app://form?step=2".into())
        );
    }

    #[test]
    fn repeated_failures_send_the_window_home() {
        let start = Instant::now();
        let mut recovery = CrashRecovery::new("app://home", Duration::from_secs(60), 2);
        recovery.page_loaded("app://form");
        assert!(matches!(recovery.page_failed(start), Recovery::Reload(_)));
        assert!(matches!(
            recovery.page_failed(start + Duration::from_secs(1)),
            Recovery::Reload(_)
        ));
        assert_eq!(
            recovery.page_failed(start + Duration::from_secs(2)),
            Recovery::Home
        );
        // The good page is forgotten, so the next failure does not return to the bad one.
        assert_eq!(
            recovery.page_failed(start + Duration::from_secs(3)),
            Recovery::Reload("app://home".into())
        );
    }

    #[test]
    fn old_failures_leave_the_window() {
        let start = Instant::now();
        let mut recovery = CrashRecovery::new("app://home", Duration::from_secs(60), 1);
        assert!(matches!(recovery.page_failed(start), Recovery::Reload(_)));
        assert!(matches!(
            recovery.page_failed(start + Duration::from_secs(600)),
            Recovery::Reload(_)
        ));
    }
}
