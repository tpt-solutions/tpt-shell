// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `Clock` trait (spec §10.5). Code that needs time takes a `Clock`, so tests can control it and
//! record/replay can virtualise it later.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Source of monotonic and wall-clock time.
pub trait Clock: Send + Sync {
    /// Time since an arbitrary fixed point. Never goes backwards.
    fn monotonic(&self) -> Duration;
    /// Time since the Unix epoch. May jump when the system clock changes.
    fn wall(&self) -> SystemTime;
}

/// The operating system's clocks.
#[derive(Debug)]
pub struct SystemClock {
    started: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        SystemClock {
            started: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        self.started.elapsed()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A clock that only moves when told to. For tests.
#[derive(Debug, Default)]
pub struct FakeClock {
    monotonic_nanos: AtomicU64,
    wall_nanos: AtomicU64,
}

impl FakeClock {
    /// Starts at zero for the monotonic clock and at the Unix epoch for the wall clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// Moves both clocks forward by `by`.
    pub fn advance(&self, by: Duration) {
        let nanos = by.as_nanos() as u64;
        self.monotonic_nanos.fetch_add(nanos, Ordering::SeqCst);
        self.wall_nanos.fetch_add(nanos, Ordering::SeqCst);
    }

    /// Moves only the wall clock, as a system time change would. The monotonic clock is unaffected.
    pub fn set_wall(&self, at: SystemTime) {
        let nanos = at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        self.wall_nanos.store(nanos, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> Duration {
        Duration::from_nanos(self.monotonic_nanos.load(Ordering::SeqCst))
    }

    fn wall(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_nanos(self.wall_nanos.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_starts_at_zero() {
        let clock = FakeClock::new();
        assert_eq!(clock.monotonic(), Duration::ZERO);
        assert_eq!(clock.wall(), UNIX_EPOCH);
    }

    #[test]
    fn advance_moves_both_clocks() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_millis(1500));
        clock.advance(Duration::from_millis(500));
        assert_eq!(clock.monotonic(), Duration::from_secs(2));
        assert_eq!(clock.wall(), UNIX_EPOCH + Duration::from_secs(2));
    }

    #[test]
    fn wall_change_leaves_monotonic_alone() {
        let clock = FakeClock::new();
        clock.advance(Duration::from_secs(1));
        clock.set_wall(UNIX_EPOCH + Duration::from_secs(1_000_000));
        assert_eq!(clock.monotonic(), Duration::from_secs(1));
        assert_eq!(clock.wall(), UNIX_EPOCH + Duration::from_secs(1_000_000));
    }

    #[test]
    fn system_clock_monotonic_never_goes_back() {
        let clock = SystemClock::new();
        let first = clock.monotonic();
        let second = clock.monotonic();
        assert!(second >= first);
    }

    #[test]
    fn system_clock_wall_is_after_the_epoch() {
        assert!(SystemClock::new().wall() > UNIX_EPOCH);
    }
}
