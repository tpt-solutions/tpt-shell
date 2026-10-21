// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Mutation driver for the fuzz targets, for machines without a fuzzing engine.
//!
//! `cargo run --release -- <target> <seconds>` mutates the seed inputs for the given time and
//! runs the target on each input. An input that makes the target panic is written to
//! `crashes/` beside this package, with the target's name, and the driver exits with status 1.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tpt_shell_fuzz::{run_target, seeds, Rng, TARGETS};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(target) = args
        .get(1)
        .map(String::as_str)
        .filter(|t| TARGETS.contains(t))
    else {
        eprintln!("usage: fuzz <{}> [seconds]", TARGETS.join("|"));
        return ExitCode::from(2);
    };
    let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    let mut rng = Rng::new(nanos);
    let seeds = seeds();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut runs: u64 = 0;

    while Instant::now() < deadline {
        // One run in eight is random bytes; the rest are mutations of a realistic seed.
        let input = if rng.next_u64().is_multiple_of(8) {
            rng.bytes(512)
        } else {
            let seed = &seeds[(rng.next_u64() as usize) % seeds.len()];
            rng.mutate(seed)
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| run_target(target, &input)));
        runs += 1;
        if outcome.is_err() {
            let path = crash_path(target, runs);
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, &input);
            eprintln!(
                "{target}: panic on input after {runs} runs; saved to {}",
                path.display()
            );
            return ExitCode::from(1);
        }
    }
    println!("{target}: {runs} runs in {seconds}s, no panic");
    ExitCode::SUCCESS
}

/// Where a crashing input is saved: `crashes/<target>-<run>.bin` in this package.
fn crash_path(target: &str, run: u64) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("crashes")
        .join(format!("{target}-{run}.bin"))
}
