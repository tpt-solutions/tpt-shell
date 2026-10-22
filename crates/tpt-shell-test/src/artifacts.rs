// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Artifact collection for `tpt-shell test` (spec §10.6).
//!
//! A test run writes its screenshots, replays and logs through a [`Collector`]. Each file goes
//! into `<root>/<test>/`, and the collector writes `<root>/index.json` listing every file with
//! its kind, size and CRC-32. CI uploads the root directory, and `tpt-shell visual compare`
//! reads the screenshots back from it.
//!
//! The CLI command that runs tests is the CLI agent's. It calls this module through
//! [`Collector`]; the hook is `Collector::new`, `add` and `finish`.
//!
//! Support: Full. Test and file names are limited to letters, digits, `-`, `_` and `.`, so an
//! artifact cannot be written outside its test's directory.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::png::crc32;

/// What an artifact is, which decides how the report shows it.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Screenshot,
    Replay,
    Log,
    Other,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Screenshot => "screenshot",
            Kind::Replay => "replay",
            Kind::Log => "log",
            Kind::Other => "other",
        }
    }
}

/// One file in the index.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub test: String,
    pub kind: Kind,
    /// The path relative to the root, with `/` separators.
    pub path: String,
    pub bytes: u64,
    pub crc32: u32,
}

/// Writes artifacts for a run and the index that lists them.
///
/// Support: Full.
pub struct Collector {
    root: PathBuf,
    entries: Vec<Entry>,
}

impl Collector {
    /// Starts a collection in `root`, creating it if needed.
    ///
    /// Support: Full.
    pub fn new(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
        Ok(Collector {
            root: root.to_path_buf(),
            entries: Vec::new(),
        })
    }

    /// Writes one artifact under `<root>/<test>/<file>`.
    ///
    /// Support: Full.
    pub fn add(&mut self, test: &str, kind: Kind, file: &str, bytes: &[u8]) -> Result<(), String> {
        if !is_plain(test) || !is_plain(file) {
            return Err(format!("`{test}/{file}` is not a plain name"));
        }
        let dir = self.root.join(test);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let target = dir.join(file);
        fs::write(&target, bytes).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        self.entries.push(Entry {
            test: test.to_string(),
            kind,
            path: format!("{test}/{file}"),
            bytes: bytes.len() as u64,
            crc32: crc32(&[bytes]),
        });
        Ok(())
    }

    /// The artifacts added so far.
    ///
    /// Support: Full.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Writes `index.json` and returns its path.
    ///
    /// Support: Full.
    pub fn finish(self) -> Result<PathBuf, String> {
        let items: Vec<Value> = self
            .entries
            .iter()
            .map(|e| {
                json!({
                    "test": e.test,
                    "kind": e.kind.as_str(),
                    "path": e.path,
                    "bytes": e.bytes,
                    "crc32": format!("{:08x}", e.crc32),
                })
            })
            .collect();
        let index = json!({ "format": "tpt-shell-artifacts", "version": 1, "files": items });
        let path = self.root.join("index.json");
        fs::write(
            &path,
            serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    }
}

/// A name of 1 to 100 ASCII letters, digits, `-`, `_` or `.`, that is not `.` or `..`.
///
/// Support: Full.
pub fn is_plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-shell-artifacts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn artifacts_are_written_and_indexed() {
        let root = scratch("index");
        let mut c = Collector::new(&root).unwrap();
        c.add("login", Kind::Screenshot, "home.png", b"png-bytes")
            .unwrap();
        c.add("login", Kind::Log, "run.log", b"hello").unwrap();
        assert_eq!(c.entries().len(), 2);
        let index = c.finish().unwrap();
        assert_eq!(
            fs::read(root.join("login").join("home.png")).unwrap(),
            b"png-bytes"
        );
        let text = fs::read_to_string(index).unwrap();
        assert!(text.contains("\"path\": \"login/home.png\""));
        assert!(text.contains("\"kind\": \"screenshot\""));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_cannot_point_outside_the_root() {
        let root = scratch("names");
        let mut c = Collector::new(&root).unwrap();
        assert!(c.add("..", Kind::Other, "x", b"").is_err());
        assert!(c.add("ok", Kind::Other, "../x", b"").is_err());
        assert!(c.add("ok", Kind::Other, "a/b", b"").is_err());
        assert!(c.add("", Kind::Other, "x", b"").is_err());
        assert!(c.entries().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_index_records_a_checksum_per_file() {
        let root = scratch("crc");
        let mut c = Collector::new(&root).unwrap();
        c.add("t", Kind::Replay, "run.tptreplay", b"123456789")
            .unwrap();
        assert_eq!(c.entries()[0].crc32, 0xCBF4_3926);
        let _ = fs::remove_dir_all(&root);
    }
}
