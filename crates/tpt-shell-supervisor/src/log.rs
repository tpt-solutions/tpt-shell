// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Supervisor log file (spec M6). Each line gets a Unix timestamp. When the file would pass its
//! size limit, it moves to `<name>.1` (replacing an older `.1`) and a new file starts.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Support: Full.
pub struct FileLog {
    path: PathBuf,
    max_bytes: u64,
}

impl FileLog {
    /// Support: Full.
    pub fn new(path: impl Into<PathBuf>, max_bytes: u64) -> Self {
        FileLog {
            path: path.into(),
            max_bytes,
        }
    }

    /// Appends one line, rotating first when the file is full.
    ///
    /// Support: Full.
    pub fn line(&self, text: &str) -> io::Result<()> {
        let secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!("{secs} {text}\n");
        if self.size() + line.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())
    }

    fn size(&self) -> u64 {
        fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    fn rotate(&self) -> io::Result<()> {
        let old = rotated_name(&self.path);
        match fs::rename(&self.path, &old) {
            Ok(()) => Ok(()),
            // Nothing to rotate yet.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// `app.log` becomes `app.log.1`.
///
/// Support: Full.
pub fn rotated_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_and_the_file_rotates_at_the_limit() {
        let dir = std::env::temp_dir().join(format!("tpt-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("supervisor.log");
        let log = FileLog::new(&path, 80);

        log.line("first").expect("write");
        log.line("second").expect("write");
        let before = fs::read_to_string(&path).expect("read");
        assert!(before.contains("first") && before.contains("second"));

        // The next line would pass 80 bytes, so the old file moves aside.
        log.line("third line, long enough to start a new file")
            .expect("write");
        let rotated = fs::read_to_string(rotated_name(&path)).expect("rotated file");
        assert!(rotated.contains("second"));
        let current = fs::read_to_string(&path).expect("current file");
        assert!(current.contains("third") && !current.contains("first"));

        let _ = fs::remove_dir_all(&dir);
    }
}
