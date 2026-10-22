// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Fault-injection harness for the supervisor (spec §10.1, M6). The supervised "app" is this
//! test binary, run again with `--exact fault_app_child`. Its behaviour for each run comes from
//! a script in an environment variable, so the real process runs, beats, crashes, hangs and
//! exits as the script says:
//!
//! - `crash`: exits with a failure at once, without beating.
//! - `hang`: beats once, then stops beating. The supervisor must stop it as hung.
//! - `page_failure`: keeps beating while a page would be recovered, then exits cleanly. A
//!   recovered page failure must not cause a restart.
//! - `clean`: beats once and exits with success.
//!
//! The runs are real processes with a real heartbeat file. Timings are short so the suite stays
//! quick. The 72-hour soak is not part of this harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tpt_shell_supervisor::alert::Alert;
use tpt_shell_supervisor::log::FileLog;
use tpt_shell_supervisor::process::ProcessSpawner;
use tpt_shell_supervisor::{supervise, Backoff, CrashLoopGuard, Heartbeat, Scheduled};

/// The tests set process environment variables, so they run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// The minute of day the scheduled-restart test controls.
static MINUTE: AtomicU16 = AtomicU16::new(0);

fn minute() -> u16 {
    MINUTE.load(Ordering::SeqCst)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tpt-fault-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The child: runs the next behaviour from the script, and records that it ran.
/// It does nothing unless the harness set the script, so a normal test run skips it.
#[test]
fn fault_app_child() {
    let Ok(script) = std::env::var("TPT_FAULT_SCRIPT") else {
        return;
    };
    let counter = PathBuf::from(std::env::var("TPT_FAULT_COUNTER").expect("counter path"));
    let heartbeat = PathBuf::from(std::env::var("TPT_HEARTBEAT_FILE").expect("heartbeat path"));

    let run: usize = fs::read_to_string(&counter)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    fs::write(&counter, (run + 1).to_string()).expect("count the run");
    let behaviour = script.split(',').nth(run).unwrap_or("clean");

    match behaviour {
        "crash" => std::process::exit(3),
        "hang" => {
            tpt_shell_supervisor::heartbeat::beat(&heartbeat).expect("beat");
            std::thread::sleep(Duration::from_secs(60));
            std::process::exit(0);
        }
        "page_failure" => {
            let until = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < until {
                tpt_shell_supervisor::heartbeat::beat(&heartbeat).expect("beat");
                std::thread::sleep(Duration::from_millis(100));
            }
            std::process::exit(0);
        }
        _ => {
            tpt_shell_supervisor::heartbeat::beat(&heartbeat).expect("beat");
            std::process::exit(0);
        }
    }
}

/// A spawner that runs this test binary as the app, with the given fault script.
fn child_spawner(dir: &Path, script: &str, scheduled: Option<Scheduled>) -> ProcessSpawner {
    ProcessSpawner {
        program: std::env::current_exe().expect("this test binary"),
        args: [
            "--exact",
            "fault_app_child",
            "--nocapture",
            "--test-threads=1",
        ]
        .iter()
        .map(Into::into)
        .collect(),
        env: vec![
            ("TPT_FAULT_SCRIPT".into(), script.into()),
            ("TPT_FAULT_COUNTER".into(), dir.join("counter").into()),
        ],
        heartbeat_file: dir.join("heartbeat"),
        heartbeat: Heartbeat {
            timeout: Duration::from_secs(1),
        },
        startup_grace: Duration::from_secs(5),
        poll: Duration::from_millis(50),
        scheduled,
        minute_of_day: minute,
    }
}

fn quick_backoff() -> Backoff {
    Backoff {
        initial: Duration::from_millis(10),
        max: Duration::from_millis(50),
        factor: 2,
    }
}

#[test]
fn crashes_and_hangs_are_restarted_and_a_recovered_page_is_not() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("restarts");
    let mut spawner = child_spawner(&dir, "crash,hang,crash,page_failure", None);
    let mut guard = CrashLoopGuard::new(Duration::from_secs(600), 5, quick_backoff());
    let mut log = Vec::new();
    let mut alerts = Vec::new();

    let runs = supervise(
        &mut spawner,
        &mut guard,
        Instant::now,
        std::thread::sleep,
        &mut |line| log.push(line),
        &mut |m| alerts.push(m.to_string()),
    );

    assert_eq!(
        runs, 4,
        "three faults, then the page-failure run exits cleanly"
    );
    assert!(
        alerts.is_empty(),
        "no alert for recovered faults: {alerts:?}"
    );
    let text = log.join("\n");
    assert_eq!(text.matches("crashed").count(), 2, "{text}");
    assert_eq!(text.matches("stopped responding").count(), 1, "{text}");
    assert!(text.contains("run 4 exited cleanly"), "{text}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_loop_gives_up_once_and_the_alert_reaches_its_file() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("loop");
    let mut spawner = child_spawner(&dir, "crash,crash,crash,crash,crash", None);
    let mut guard = CrashLoopGuard::new(Duration::from_secs(600), 2, quick_backoff());
    let alert_file = dir.join("alerts.txt");
    let log_file = FileLog::new(dir.join("supervisor.log"), 1024 * 1024);
    let alert = Alert::File(alert_file.clone());
    let mut log = Vec::new();
    let mut alerts = Vec::new();

    let runs = supervise(
        &mut spawner,
        &mut guard,
        Instant::now,
        std::thread::sleep,
        &mut |line| {
            log_file.line(&line).expect("log line");
            log.push(line);
        },
        &mut |m| {
            alert.raise(m).expect("alert");
            alerts.push(m.to_string());
        },
    );

    assert_eq!(runs, 3, "two restarts, then the third crash gives up");
    assert_eq!(alerts.len(), 1);
    let alert_text = fs::read_to_string(&alert_file).expect("alert file");
    assert_eq!(alert_text.lines().count(), 1);
    assert!(alert_text.contains("giving up"));
    let logged = fs::read_to_string(dir.join("supervisor.log")).expect("log file");
    assert!(logged.contains("giving up"), "{logged}");
    assert_eq!(logged.lines().count(), log.len(), "every event is logged");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_scheduled_restart_stops_the_run_without_counting_a_crash() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("scheduled");
    MINUTE.store(9 * 60 + 59, Ordering::SeqCst);
    // The app would run for 1.5 s and exit cleanly. The restart at 10:00 stops it first.
    let mut spawner = child_spawner(
        &dir,
        "page_failure,clean",
        Some(Scheduled {
            minute_of_day: 10 * 60,
        }),
    );
    let mut guard = CrashLoopGuard::new(Duration::from_secs(600), 0, quick_backoff());
    let mut log = Vec::new();
    let mut alerts = Vec::new();

    let flip = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        MINUTE.store(10 * 60, Ordering::SeqCst);
    });
    let runs = supervise(
        &mut spawner,
        &mut guard,
        Instant::now,
        std::thread::sleep,
        &mut |line| log.push(line),
        &mut |m| alerts.push(m.to_string()),
    );
    flip.join().expect("flip thread");

    assert_eq!(
        runs, 2,
        "the scheduled stop, then the second run exits cleanly"
    );
    assert!(alerts.is_empty(), "a scheduled restart is not a crash");
    assert!(log[0].contains("scheduled restart"), "{log:?}");
    let _ = fs::remove_dir_all(&dir);
}
