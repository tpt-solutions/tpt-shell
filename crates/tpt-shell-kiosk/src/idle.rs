// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Idle reset (spec §10.1). After a period with no input, a kiosk goes back to its home page
//! and clears the session. The timer only tracks time; the caller acts when it is due.

use std::time::{Duration, Instant};

/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleTimer {
    limit: Duration,
    last_activity: Instant,
}

impl IdleTimer {
    /// A timer that becomes due `limit` after `now`, unless it is touched first.
    ///
    /// Support: Full.
    pub fn new(limit: Duration, now: Instant) -> Self {
        IdleTimer {
            limit,
            last_activity: now,
        }
    }

    /// Records user input at `now`.
    ///
    /// Support: Full.
    pub fn touch(&mut self, now: Instant) {
        self.last_activity = now;
    }

    /// How long there has been no input at `now`.
    ///
    /// Support: Full.
    pub fn idle_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_activity)
    }

    /// Whether the kiosk should reset at `now`.
    ///
    /// Support: Full.
    pub fn is_due(&self, now: Instant) -> bool {
        self.idle_for(now) >= self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_timer_is_due_after_the_limit_and_touching_resets_it() {
        let start = Instant::now();
        let mut timer = IdleTimer::new(Duration::from_secs(60), start);
        assert!(!timer.is_due(start + Duration::from_secs(59)));
        assert!(timer.is_due(start + Duration::from_secs(60)));

        timer.touch(start + Duration::from_secs(50));
        assert!(!timer.is_due(start + Duration::from_secs(100)));
        assert_eq!(
            timer.idle_for(start + Duration::from_secs(100)),
            Duration::from_secs(50)
        );
        assert!(timer.is_due(start + Duration::from_secs(111)));
    }
}
