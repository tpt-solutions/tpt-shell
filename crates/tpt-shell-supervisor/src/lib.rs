// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Restart policy for a supervised app (spec §10.1, M6). The decisions are pure functions of
//! the times they are given, so they can be tested without waiting. [`supervise`] runs the
//! loop over a [`Spawner`], which the platform implements with real processes.
//!
//! - [`Backoff`]: the wait before restart grows exponentially and is capped.
//! - [`CrashLoopGuard`]: counts crashes inside a window. Too many means stop and alert.
//! - [`Heartbeat`]: a hang is a missed heartbeat past a timeout.
//! - [`Scheduled`]: a daily restart at a fixed time of day.

use std::time::{Duration, Instant};

/// Support: Full.
pub mod alert;
/// Support: Full.
pub mod heartbeat;
/// Support: Partial. The Windows Task Scheduler path is tested on Windows. The macOS LaunchAgent
/// and Linux systemd output is not run on those systems here (compile-checked only).
pub mod install;
/// Support: Full.
pub mod log;
/// Support: Partial. Spawning and stopping is tested on Windows only; on macOS and Linux it is
/// compile-checked, not run here.
pub mod process;

/// Exponential backoff: `initial`, then `initial * factor`, and so on, never above `max`.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Backoff {
    pub initial: Duration,
    pub max: Duration,
    pub factor: u32,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            factor: 2,
        }
    }
}

impl Backoff {
    /// Delay before restart number `attempt` (0 is the first restart).
    ///
    /// Support: Full.
    pub fn delay(&self, attempt: u32) -> Duration {
        let mut delay = self.initial;
        for _ in 0..attempt {
            delay = delay.saturating_mul(self.factor.max(1));
            if delay >= self.max {
                return self.max;
            }
        }
        delay.min(self.max)
    }
}

/// What to do after a crash.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Restart after this delay.
    Restart(Duration),
    /// Too many crashes in the window. Stop, and raise the alert.
    GiveUp,
}

/// Counts recent crashes. A crash older than `window` no longer counts.
///
/// Support: Full.
#[derive(Clone, Debug)]
pub struct CrashLoopGuard {
    pub window: Duration,
    pub max_crashes: u32,
    pub backoff: Backoff,
    crashes: Vec<Instant>,
}

impl CrashLoopGuard {
    /// Support: Full.
    pub fn new(window: Duration, max_crashes: u32, backoff: Backoff) -> Self {
        CrashLoopGuard {
            window,
            max_crashes,
            backoff,
            crashes: Vec::new(),
        }
    }

    /// Records a crash at `now` and returns the decision.
    ///
    /// Support: Full.
    pub fn record_crash(&mut self, now: Instant) -> Decision {
        let window = self.window;
        self.crashes
            .retain(|t| now.saturating_duration_since(*t) < window);
        self.crashes.push(now);
        let recent = self.crashes.len() as u32;
        if recent > self.max_crashes {
            Decision::GiveUp
        } else {
            // The first crash in a window restarts after the initial delay, then the delay grows.
            Decision::Restart(self.backoff.delay(recent - 1))
        }
    }
}

/// Heartbeat watch: the app must report at least once per `timeout`.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heartbeat {
    pub timeout: Duration,
}

impl Heartbeat {
    /// Support: Full.
    pub fn is_hung(&self, last_beat: Instant, now: Instant) -> bool {
        now.saturating_duration_since(last_beat) > self.timeout
    }
}

/// A restart at a fixed time of day, in minutes after midnight (0-1439).
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scheduled {
    pub minute_of_day: u16,
}

impl Scheduled {
    /// Whether a restart is due, given the previous minute-of-day and the current one.
    /// Works across midnight.
    ///
    /// Support: Full.
    pub fn is_due(&self, previous: u16, current: u16) -> bool {
        if previous == current {
            return false;
        }
        if previous < current {
            previous < self.minute_of_day && self.minute_of_day <= current
        } else {
            // Crossed midnight.
            self.minute_of_day > previous || self.minute_of_day <= current
        }
    }
}

/// How one run of the app ended.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Exited with success. The loop stops.
    Clean,
    /// Exited with a failure, or could not be started. Counts as a crash.
    Crashed,
    /// Still running, but its heartbeat stopped. It was stopped. Counts as a crash.
    Hung,
    /// Stopped for the daily restart. Does not count as a crash.
    ScheduledRestart,
}

/// Starts the app and reports how it ended. Platforms implement this with real processes.
///
/// Support: Full.
pub trait Spawner {
    fn run_once(&mut self) -> Exit;
}

/// Runs the app until it exits cleanly or the guard gives up. Every restart and the give-up
/// are passed to `log` as they happen, and `alert` is called once when the loop gives up.
/// `sleep` waits for the backoff. Returns the number of runs.
///
/// Support: Full.
pub fn supervise(
    spawner: &mut dyn Spawner,
    guard: &mut CrashLoopGuard,
    mut clock: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
    log: &mut dyn FnMut(String),
    alert: &mut dyn FnMut(&str),
) -> u32 {
    let mut runs = 0;
    loop {
        runs += 1;
        let reason = match spawner.run_once() {
            Exit::Clean => {
                log(format!("run {runs} exited cleanly"));
                return runs;
            }
            Exit::ScheduledRestart => {
                log(format!("run {runs} stopped for the scheduled restart"));
                continue;
            }
            Exit::Crashed => "crashed",
            Exit::Hung => "stopped responding",
        };
        let now = clock();
        match guard.record_crash(now) {
            Decision::Restart(delay) => {
                log(format!("run {runs} {reason}; restarting after {delay:?}"));
                sleep(delay);
            }
            Decision::GiveUp => {
                let message = format!(
                    "crash loop: {} crashes within {:?}; giving up",
                    guard.max_crashes + 1,
                    guard.window
                );
                log(message.clone());
                alert(&message);
                return runs;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn backoff_grows_then_caps() {
        let b = Backoff {
            initial: secs(1),
            max: secs(10),
            factor: 2,
        };
        let delays: Vec<u64> = (0..6).map(|a| b.delay(a).as_secs()).collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 10, 10]);
    }

    #[test]
    fn backoff_never_overflows_on_huge_attempt_numbers() {
        let b = Backoff::default();
        assert_eq!(b.delay(u32::MAX), b.max);
    }

    #[test]
    fn crash_loop_gives_up_past_the_cap_inside_the_window() {
        let start = Instant::now();
        let mut g = CrashLoopGuard::new(secs(60), 2, Backoff::default());
        assert!(matches!(g.record_crash(start), Decision::Restart(_)));
        assert!(matches!(
            g.record_crash(start + secs(1)),
            Decision::Restart(_)
        ));
        assert_eq!(g.record_crash(start + secs(2)), Decision::GiveUp);
    }

    #[test]
    fn old_crashes_leave_the_window() {
        let start = Instant::now();
        let mut g = CrashLoopGuard::new(secs(60), 1, Backoff::default());
        assert!(matches!(g.record_crash(start), Decision::Restart(_)));
        // Well after the window, the earlier crash no longer counts.
        assert!(matches!(
            g.record_crash(start + secs(600)),
            Decision::Restart(_)
        ));
    }

    #[test]
    fn heartbeat_timeout_marks_a_hang() {
        let hb = Heartbeat { timeout: secs(5) };
        let t = Instant::now();
        assert!(!hb.is_hung(t, t + secs(5)));
        assert!(hb.is_hung(t, t + secs(6)));
    }

    #[test]
    fn scheduled_restart_fires_once_including_across_midnight() {
        let at_0300 = Scheduled { minute_of_day: 180 };
        assert!(at_0300.is_due(179, 180));
        assert!(!at_0300.is_due(180, 181));
        assert!(!at_0300.is_due(100, 170));
        let at_0005 = Scheduled { minute_of_day: 5 };
        assert!(at_0005.is_due(1438, 6), "crossing midnight");
        assert!(!at_0005.is_due(1400, 1430));
    }

    struct Script(Vec<Exit>, usize);

    impl Spawner for Script {
        fn run_once(&mut self) -> Exit {
            let exit = self.0.get(self.1).copied().unwrap_or(Exit::Crashed);
            self.1 += 1;
            exit
        }
    }

    #[test]
    fn a_clean_exit_stops_the_loop_without_restarting() {
        let mut spawner = Script(vec![Exit::Crashed, Exit::Clean], 0);
        let mut guard = CrashLoopGuard::new(secs(60), 5, Backoff::default());
        let mut log = Vec::new();
        let mut alerts = Vec::new();
        let mut waits = Vec::new();
        let runs = supervise(
            &mut spawner,
            &mut guard,
            Instant::now,
            |d| waits.push(d),
            &mut |line| log.push(line),
            &mut |m| alerts.push(m.to_string()),
        );
        assert_eq!(runs, 2);
        assert_eq!(waits, vec![secs(1)]);
        assert!(alerts.is_empty());
    }

    #[test]
    fn a_crash_loop_stops_and_alerts_once() {
        let mut spawner = Script(vec![], 0);
        let mut guard = CrashLoopGuard::new(secs(600), 2, Backoff::default());
        let mut log = Vec::new();
        let mut alerts = Vec::new();
        let runs = supervise(
            &mut spawner,
            &mut guard,
            Instant::now,
            |_| {},
            &mut |line| log.push(line),
            &mut |m| alerts.push(m.to_string()),
        );
        assert_eq!(runs, 3, "two restarts, then the third crash gives up");
        assert_eq!(alerts.len(), 1);
        assert!(alerts[0].contains("giving up"));
        assert!(log.last().unwrap().contains("giving up"));
    }
}
