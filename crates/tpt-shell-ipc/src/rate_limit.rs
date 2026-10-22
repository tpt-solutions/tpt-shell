// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Per-window token bucket for page commands (spec §5.5).
//!
//! The caller owns one [`RateLimiter`] per window and passes the current time in, so the
//! limiter itself makes no clock calls and tests are deterministic.

use std::time::Duration;

/// How many commands a window may send: `burst` at once, refilled at `per_second`.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateLimit {
    pub burst: u32,
    pub per_second: u32,
}

impl Default for RateLimit {
    fn default() -> Self {
        RateLimit {
            burst: 100,
            per_second: 50,
        }
    }
}

/// Token bucket. A bucket starts full.
///
/// Support: Full.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    limit: RateLimit,
    tokens: f64,
    last: Option<Duration>,
}

impl RateLimiter {
    /// Support: Full.
    pub fn new(limit: RateLimit) -> Self {
        RateLimiter {
            limit,
            tokens: f64::from(limit.burst),
            last: None,
        }
    }

    /// Takes one token if one is available at time `now`. `now` must come from one monotonic
    /// source; a time earlier than the last call refills nothing.
    ///
    /// Support: Full.
    pub fn allow(&mut self, now: Duration) -> bool {
        if let Some(last) = self.last {
            let elapsed = now.saturating_sub(last).as_secs_f64();
            let refilled = self.tokens + elapsed * f64::from(self.limit.per_second);
            self.tokens = refilled.min(f64::from(self.limit.burst));
        }
        self.last = Some(now);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn burst_is_allowed_then_refused() {
        let mut l = RateLimiter::new(RateLimit {
            burst: 3,
            per_second: 1,
        });
        assert!(l.allow(ms(0)));
        assert!(l.allow(ms(0)));
        assert!(l.allow(ms(0)));
        assert!(!l.allow(ms(0)));
    }

    #[test]
    fn tokens_refill_at_the_configured_rate() {
        let mut l = RateLimiter::new(RateLimit {
            burst: 1,
            per_second: 10,
        });
        assert!(l.allow(ms(0)));
        assert!(!l.allow(ms(50)));
        // 100 ms at 10/s refills one token.
        assert!(l.allow(ms(150)));
        assert!(!l.allow(ms(150)));
    }

    #[test]
    fn refill_is_capped_at_the_burst() {
        let mut l = RateLimiter::new(RateLimit {
            burst: 2,
            per_second: 100,
        });
        assert!(l.allow(ms(0)));
        // A long idle period must not bank more than `burst` tokens.
        assert!(l.allow(ms(60_000)));
        assert!(l.allow(ms(60_000)));
        assert!(!l.allow(ms(60_000)));
    }

    #[test]
    fn time_going_backwards_refills_nothing() {
        let mut l = RateLimiter::new(RateLimit {
            burst: 1,
            per_second: 1000,
        });
        assert!(l.allow(ms(100)));
        assert!(!l.allow(ms(50)));
    }
}
