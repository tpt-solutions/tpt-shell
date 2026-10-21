// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runs the app as a child process and watches it (spec §10.1). The child is started with the
//! heartbeat file in `TPT_HEARTBEAT_FILE`. It is stopped when it exits, when it stops beating,
//! or at the scheduled restart time.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use crate::{heartbeat, Exit, Heartbeat, Scheduled, Spawner};

pub struct ProcessSpawner {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub heartbeat_file: PathBuf,
    pub heartbeat: Heartbeat,
    /// How long a new run may take to send its first beat.
    pub startup_grace: Duration,
    /// How often the child is checked.
    pub poll: Duration,
    /// A restart at this time of day, if any.
    pub scheduled: Option<Scheduled>,
    /// The current minute of the day, in local time. The platform supplies it.
    pub minute_of_day: fn() -> u16,
}

impl ProcessSpawner {
    fn missed_heartbeat(&self, started: Instant) -> bool {
        match heartbeat::age(&self.heartbeat_file) {
            Some(age) => age > self.heartbeat.timeout,
            None => started.elapsed() > self.startup_grace,
        }
    }
}

impl Spawner for ProcessSpawner {
    fn run_once(&mut self) -> Exit {
        // A beat from an earlier run must not count for this one.
        let _ = std::fs::remove_file(&self.heartbeat_file);
        let mut child = match Command::new(&self.program)
            .args(&self.args)
            .env(heartbeat::ENV, &self.heartbeat_file)
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return Exit::Crashed,
        };
        let started = Instant::now();
        let mut minute = (self.minute_of_day)();
        loop {
            std::thread::sleep(self.poll);
            match child.try_wait() {
                Ok(Some(status)) => return exit_of(status),
                Ok(None) => {}
                Err(_) => {
                    stop(&mut child);
                    return Exit::Crashed;
                }
            }
            if let Some(schedule) = self.scheduled {
                let now = (self.minute_of_day)();
                if schedule.is_due(minute, now) {
                    stop(&mut child);
                    return Exit::ScheduledRestart;
                }
                minute = now;
            }
            if self.missed_heartbeat(started) {
                stop(&mut child);
                return Exit::Hung;
            }
        }
    }
}

fn exit_of(status: ExitStatus) -> Exit {
    if status.success() {
        Exit::Clean
    } else {
        Exit::Crashed
    }
}

/// Kills a child that is still running and reaps it.
fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
