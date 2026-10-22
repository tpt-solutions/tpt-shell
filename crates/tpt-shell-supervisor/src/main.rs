// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell-supervisor`: runs a tpt-shell app and restarts it when it crashes, hangs, or is
//! due for its daily restart (spec §10.1).
//!
//! usage: tpt-shell-supervisor [options] -- <program> [args...]
//!
//! options:
//!   --heartbeat-timeout <secs>   a run that has not beaten for this long is hung (default 30)
//!   --startup-grace <secs>       time a run has to send its first beat (default 60)
//!   --restart-at <HH:MM>         restart once a day at this local time
//!   --max-crashes <n>            crashes allowed inside the window before giving up (default 5)
//!   --crash-window <secs>        the window for counting crashes (default 300)
//!   --log <file>                 append a timestamped line per event; rotated at 1 MiB
//!   --alert-command <program>    run this with the message when giving up
//!   --alert-file <file>          append the message as a line when giving up
//!
//! The app finds its heartbeat file in the TPT_HEARTBEAT_FILE environment variable.
//!
//! Exit codes: 0 the app exited cleanly, 1 the supervisor gave up, 2 bad usage.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tpt_shell_supervisor::alert::Alert;
use tpt_shell_supervisor::log::FileLog;
use tpt_shell_supervisor::process::ProcessSpawner;
use tpt_shell_supervisor::{
    supervise, Backoff, CrashLoopGuard, Exit, Heartbeat, Scheduled, Spawner,
};

const USAGE: &str = "usage: tpt-shell-supervisor [options] -- <program> [args...]
  --heartbeat-timeout <secs>  --startup-grace <secs>  --restart-at <HH:MM>
  --max-crashes <n>  --crash-window <secs>  --log <file>
  --alert-command <program> | --alert-file <file>";

const LOG_LIMIT: u64 = 1024 * 1024;

struct Options {
    heartbeat_timeout: Duration,
    startup_grace: Duration,
    scheduled: Option<Scheduled>,
    max_crashes: u32,
    crash_window: Duration,
    log: Option<PathBuf>,
    alert: Option<Alert>,
    program: OsString,
    args: Vec<OsString>,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("tpt-shell-supervisor: {message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if run(options) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Runs the supervision loop. Returns true when the app exited cleanly.
fn run(options: Options) -> bool {
    let log = options
        .log
        .as_ref()
        .map(|path| FileLog::new(path, LOG_LIMIT));
    let heartbeat_file =
        std::env::temp_dir().join(format!("tpt-shell-heartbeat-{}", std::process::id()));
    let mut spawner = Recording {
        inner: ProcessSpawner {
            program: PathBuf::from(&options.program),
            args: options.args,
            env: Vec::new(),
            heartbeat_file: heartbeat_file.clone(),
            heartbeat: Heartbeat {
                timeout: options.heartbeat_timeout,
            },
            startup_grace: options.startup_grace,
            poll: Duration::from_millis(200),
            scheduled: options.scheduled,
            minute_of_day: local_minute_of_day,
        },
        last: None,
    };
    let mut guard = CrashLoopGuard::new(
        options.crash_window,
        options.max_crashes,
        Backoff::default(),
    );
    let mut emit = |line: String| {
        println!("{line}");
        if let Some(log) = &log {
            if let Err(e) = log.line(&line) {
                eprintln!("tpt-shell-supervisor: cannot write the log: {e}");
            }
        }
    };
    let mut alert = |message: &str| {
        if let Some(target) = &options.alert {
            if let Err(e) = target.raise(message) {
                eprintln!("tpt-shell-supervisor: alert failed: {e}");
            }
        }
    };
    supervise(
        &mut spawner,
        &mut guard,
        Instant::now,
        std::thread::sleep,
        &mut emit,
        &mut alert,
    );
    let _ = std::fs::remove_file(&heartbeat_file);
    spawner.last == Some(Exit::Clean)
}

/// Remembers how the last run ended, so the caller can report the outcome.
struct Recording {
    inner: ProcessSpawner,
    last: Option<Exit>,
}

impl Spawner for Recording {
    fn run_once(&mut self) -> Exit {
        let exit = self.inner.run_once();
        self.last = Some(exit);
        exit
    }
}

/// The current minute of the day in local time (Windows), or UTC elsewhere.
fn local_minute_of_day() -> u16 {
    #[cfg(windows)]
    {
        tpt_shell_platform_windows::local_time::minute_of_day()
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        ((secs / 60) % (24 * 60)) as u16
    }
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let split = args
        .iter()
        .position(|a| a == "--")
        .ok_or("missing `--` before the program to run")?;
    let (flags, rest) = (&args[..split], &args[split + 1..]);
    let program = rest.first().ok_or("no program after `--`")?.clone();

    let mut options = Options {
        heartbeat_timeout: Duration::from_secs(30),
        startup_grace: Duration::from_secs(60),
        scheduled: None,
        max_crashes: 5,
        crash_window: Duration::from_secs(300),
        log: None,
        alert: None,
        program,
        args: rest[1..].to_vec(),
    };
    let mut i = 0;
    while i < flags.len() {
        let flag = flags[i].to_string_lossy().into_owned();
        let value = || -> Result<String, String> {
            flags
                .get(i + 1)
                .map(|v| v.to_string_lossy().into_owned())
                .ok_or(format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--heartbeat-timeout" => options.heartbeat_timeout = seconds(&value()?)?,
            "--startup-grace" => options.startup_grace = seconds(&value()?)?,
            "--crash-window" => options.crash_window = seconds(&value()?)?,
            "--max-crashes" => {
                options.max_crashes = value()?
                    .parse()
                    .map_err(|_| "--max-crashes needs a whole number".to_string())?;
            }
            "--restart-at" => options.scheduled = Some(time_of_day(&value()?)?),
            "--log" => options.log = Some(PathBuf::from(value()?)),
            "--alert-command" => options.alert = Some(Alert::Command(PathBuf::from(value()?))),
            "--alert-file" => options.alert = Some(Alert::File(PathBuf::from(value()?))),
            other => return Err(format!("unknown option {other}")),
        }
        i += 2;
    }
    Ok(options)
}

fn seconds(text: &str) -> Result<Duration, String> {
    text.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| format!("{text:?} is not a number of seconds"))
}

fn time_of_day(text: &str) -> Result<Scheduled, String> {
    let (h, m) = text.split_once(':').ok_or("--restart-at needs HH:MM")?;
    let hour: u16 = h.parse().map_err(|_| "--restart-at needs HH:MM")?;
    let minute: u16 = m.parse().map_err(|_| "--restart-at needs HH:MM")?;
    if hour > 23 || minute > 59 {
        return Err("--restart-at needs a time between 00:00 and 23:59".into());
    }
    Ok(Scheduled {
        minute_of_day: hour * 60 + minute,
    })
}
