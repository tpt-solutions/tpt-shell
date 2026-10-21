// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A stand-in app for the supervisor tests. Its first argument picks how it behaves:
//!
//! - `exit-ok`: exits with success at once.
//! - `exit-fail`: exits with failure at once.
//! - `beat`: beats its heartbeat file until it is killed.
//! - `beat-then-stop`: beats for about a second, then sleeps without beating.
//! - `hang`: sleeps without beating.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tpt_shell_supervisor::heartbeat;

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let beat_file = std::env::var_os(heartbeat::ENV).map(PathBuf::from);
    match mode.as_str() {
        "exit-ok" => {}
        "exit-fail" => std::process::exit(3),
        "beat" => beat_until_killed(beat_file.as_deref()),
        "beat-then-stop" => {
            let until = Instant::now() + Duration::from_millis(1000);
            while Instant::now() < until {
                beat(beat_file.as_deref());
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_secs(600));
        }
        "hang" => std::thread::sleep(Duration::from_secs(600)),
        other => {
            eprintln!("supervisor-child: unknown mode {other:?}");
            std::process::exit(2);
        }
    }
}

fn beat(path: Option<&std::path::Path>) {
    if let Some(path) = path {
        let _ = heartbeat::beat(path);
    }
}

fn beat_until_killed(path: Option<&std::path::Path>) -> ! {
    loop {
        beat(path);
        std::thread::sleep(Duration::from_millis(50));
    }
}
