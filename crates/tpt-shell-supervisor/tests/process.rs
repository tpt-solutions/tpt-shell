// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runs real child processes under the supervisor. The child is `supervisor-child`, built with
//! this crate, and each test picks its behaviour with an argument.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tpt_shell_supervisor::alert::Alert;
use tpt_shell_supervisor::process::ProcessSpawner;
use tpt_shell_supervisor::{
    supervise, Backoff, CrashLoopGuard, Exit, Heartbeat, Scheduled, Spawner,
};

fn child(mode: &str, timeout: Duration, grace: Duration) -> ProcessSpawner {
    let dir = std::env::temp_dir().join(format!("tpt-sup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    ProcessSpawner {
        program: PathBuf::from(env!("CARGO_BIN_EXE_supervisor-child")),
        args: vec![mode.into()],
        heartbeat_file: dir.join(format!("{mode}.beat")),
        heartbeat: Heartbeat { timeout },
        startup_grace: grace,
        poll: Duration::from_millis(50),
        scheduled: None,
        minute_of_day: || 0,
    }
}

#[test]
fn a_clean_exit_is_reported_as_clean() {
    let mut spawner = child("exit-ok", Duration::from_secs(5), Duration::from_secs(5));
    assert_eq!(spawner.run_once(), Exit::Clean);
}

#[test]
fn a_failing_exit_is_reported_as_a_crash() {
    let mut spawner = child("exit-fail", Duration::from_secs(5), Duration::from_secs(5));
    assert_eq!(spawner.run_once(), Exit::Crashed);
}

#[test]
fn a_run_that_never_beats_is_stopped_after_the_startup_grace() {
    let mut spawner = child("hang", Duration::from_secs(5), Duration::from_millis(400));
    let started = Instant::now();
    assert_eq!(spawner.run_once(), Exit::Hung);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the hung child was not killed"
    );
}

#[test]
fn a_run_that_stops_beating_is_stopped_after_the_heartbeat_timeout() {
    // The child beats for about a second, so it must not be stopped during that time.
    let mut spawner = child(
        "beat-then-stop",
        Duration::from_millis(600),
        Duration::from_secs(5),
    );
    let started = Instant::now();
    assert_eq!(spawner.run_once(), Exit::Hung);
    assert!(
        started.elapsed() >= Duration::from_millis(1000),
        "the run was stopped while it was still beating"
    );
}

static MINUTE_CALLS: AtomicU32 = AtomicU32::new(0);

/// 600 on the first call, then 601: the schedule for 601 comes due on the first poll.
fn minute_that_crosses_601() -> u16 {
    if MINUTE_CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
        600
    } else {
        601
    }
}

#[test]
fn the_scheduled_restart_stops_a_healthy_run_and_is_not_a_crash() {
    let mut spawner = child("beat", Duration::from_secs(5), Duration::from_secs(5));
    spawner.scheduled = Some(Scheduled { minute_of_day: 601 });
    spawner.minute_of_day = minute_that_crosses_601;
    assert_eq!(spawner.run_once(), Exit::ScheduledRestart);
}

#[test]
fn a_crash_loop_gives_up_and_writes_the_alert_file() {
    let dir = std::env::temp_dir().join(format!("tpt-sup-alert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let alert_file = dir.join("alerts.txt");
    let _ = std::fs::remove_file(&alert_file);
    let target = Alert::File(alert_file.clone());

    let mut spawner = child("exit-fail", Duration::from_secs(5), Duration::from_secs(5));
    let mut guard = CrashLoopGuard::new(
        Duration::from_secs(60),
        2,
        Backoff {
            initial: Duration::from_millis(1),
            max: Duration::from_millis(1),
            factor: 2,
        },
    );
    let mut lines = Vec::new();
    let runs = supervise(
        &mut spawner,
        &mut guard,
        Instant::now,
        |_| {},
        &mut |line| lines.push(line),
        &mut |message| {
            target.raise(message).expect("alert");
        },
    );
    assert_eq!(runs, 3, "two restarts, then the third crash gives up");
    let alerts = std::fs::read_to_string(&alert_file).expect("alert file");
    assert!(alerts.contains("giving up"), "{alerts}");
    assert!(lines.iter().any(|l| l.contains("restarting after")));
    let _ = std::fs::remove_dir_all(&dir);
}
