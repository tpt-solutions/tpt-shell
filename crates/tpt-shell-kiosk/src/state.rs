// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! State snapshots for a kiosk app (spec §10.1): the app saves its state on an interval and at
//! exit, and restores it at the next start. Each save writes a temporary file and then renames
//! it over the snapshot, so a crash in the middle of a save leaves the previous snapshot intact.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

/// Why a snapshot could not be restored. A missing snapshot is not an error; see [`restore`].
#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    /// The snapshot file is there but cannot be read as the expected type.
    Corrupt(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::Io(e) => write!(f, "cannot read the state snapshot: {e}"),
            StateError::Corrupt(e) => write!(f, "the state snapshot is corrupt: {e}"),
        }
    }
}

impl std::error::Error for StateError {}

/// Writes `value` to `path`, replacing any earlier snapshot.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    let tmp = temp_name(path);
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// Reads the snapshot at `path`. Returns `Ok(None)` when there is none yet, so a first start
/// begins from the app's defaults. A snapshot that cannot be read is `Err`, so the caller can
/// fall back to its defaults and log the problem.
pub fn restore<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, StateError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StateError::Io(e)),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| StateError::Corrupt(e.to_string()))
}

fn temp_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Session {
        page: String,
        zoom: u32,
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-kiosk-state-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        dir.join(name)
    }

    #[test]
    fn a_saved_state_comes_back_and_a_newer_save_replaces_it() {
        let path = scratch("session.json");
        let _ = fs::remove_file(&path);
        assert_eq!(restore::<Session>(&path).expect("missing is fine"), None);

        save(
            &path,
            &Session {
                page: "a".into(),
                zoom: 100,
            },
        )
        .expect("save");
        save(
            &path,
            &Session {
                page: "b".into(),
                zoom: 125,
            },
        )
        .expect("save again");
        let restored: Option<Session> = restore(&path).expect("restore");
        assert_eq!(
            restored,
            Some(Session {
                page: "b".into(),
                zoom: 125
            })
        );
        assert!(
            !temp_name(&path).exists(),
            "the temporary file is renamed away"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_snapshot_is_reported_not_silently_used() {
        let path = scratch("corrupt.json");
        fs::write(&path, "{ not json").expect("write");
        assert!(matches!(
            restore::<Session>(&path),
            Err(StateError::Corrupt(_))
        ));
        let _ = fs::remove_file(&path);
    }
}
