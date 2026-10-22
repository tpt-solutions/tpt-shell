// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The heartbeat file (spec §10.1). The supervisor gives the app a path in `TPT_HEARTBEAT_FILE`.
//! The app calls [`beat`] regularly, which rewrites that file. The supervisor reads how old the
//! file is. This is local IPC without a socket, so the shell opens no network connections.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Environment variable that names the heartbeat file for the app.
///
/// Support: Full.
pub const ENV: &str = "TPT_HEARTBEAT_FILE";

/// Records that the app is alive, by writing the current Unix time (in milliseconds) to `path`.
/// Writing the time, not only touching the file, makes each beat change the contents as well.
///
/// Support: Full.
pub fn beat(path: &Path) -> io::Result<()> {
    let millis = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut file: File = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    write!(file, "{millis}")
}

/// How long ago the last beat was, or `None` when the file does not exist yet.
///
/// Support: Full.
pub fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tpt-hb-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_beat_creates_the_file_and_its_age_is_small() {
        let path = scratch("beat");
        let _ = std::fs::remove_file(&path);
        assert!(age(&path).is_none());
        beat(&path).expect("beat");
        assert!(age(&path).expect("file exists") < Duration::from_secs(60));
        let _ = std::fs::remove_file(&path);
    }
}
