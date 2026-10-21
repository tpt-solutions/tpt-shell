// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The heartbeat file (spec §10.1). When the supervisor starts the app it sets `TPT_HEARTBEAT_FILE`
//! to a path. The app calls [`beat`] regularly, which rewrites that file with the current time,
//! and the supervisor reads its age. Nothing is written unless the variable is set, and no socket
//! is used.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The environment variable that names the heartbeat file.
pub const ENV: &str = "TPT_HEARTBEAT_FILE";

/// The heartbeat path from the environment, or `None` when the app is not supervised.
pub fn from_env() -> Option<PathBuf> {
    std::env::var_os(ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Records that the app is alive by writing the current Unix time in milliseconds to `path`.
pub fn beat(path: &Path) -> io::Result<()> {
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    write!(file, "{millis}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_writes_the_time_and_rewrites_it() {
        let path = std::env::temp_dir().join(format!("tpt-kiosk-hb-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        beat(&path).expect("first beat");
        let first: u128 = std::fs::read_to_string(&path)
            .expect("read")
            .parse()
            .expect("millis");
        std::thread::sleep(std::time::Duration::from_millis(5));
        beat(&path).expect("second beat");
        let second: u128 = std::fs::read_to_string(&path)
            .expect("read")
            .parse()
            .expect("millis");
        assert!(second > first);
        let _ = std::fs::remove_file(&path);
    }
}
