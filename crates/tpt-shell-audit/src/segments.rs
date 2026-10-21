// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Rotation and size limits (spec §10.3). A log is a directory of segments named
//! `<stem>.000001.jsonl`, `<stem>.000002.jsonl`, and so on. A segment is closed when it reaches
//! the size limit, and the next one starts where it stopped: its first record has the sequence
//! number and the `prev` hash that follow the last record of the segment before it. Verification
//! runs over all segments in order, so the chain is unbroken across files.
//!
//! Deleting an old segment is detected: the numbering must run 1, 2, 3, and so on without gaps.
//! The first remaining segment then fails, because its `prev` no longer matches anything.

use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;

use crate::anchor::{check_anchors, Anchor, AnchorPoint};
use crate::{verify_chain, AuditError, AuditLog, Entry, Policy, Signing, Summary};

/// Segments are missing or out of order, or there are none. Fix: restore the missing file, or
/// point verification at the directory that holds the whole log.
pub const CODE_SEGMENT: &str = "TS0104";

/// The path of segment `n` (1-based) of the log called `stem` in `dir`.
pub fn segment_path(dir: &Path, stem: &str, n: u64) -> PathBuf {
    dir.join(format!("{stem}.{n:06}.jsonl"))
}

/// The segments of the log called `stem` in `dir`, in order. Fails when a number is missing.
pub fn segment_paths(dir: &Path, stem: &str) -> Result<Vec<PathBuf>, AuditError> {
    let prefix = format!("{stem}.");
    let mut found: Vec<(u64, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(middle) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".jsonl"))
        else {
            continue;
        };
        if middle.len() == 6 && middle.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = middle.parse::<u64>() {
                found.push((n, path));
            }
        }
    }
    found.sort_by_key(|(n, _)| *n);
    for (i, (n, _)) in found.iter().enumerate() {
        if *n != i as u64 + 1 {
            return Err(AuditError::Refused {
                code: CODE_SEGMENT,
                reason: format!("segment {} of `{stem}` is missing", i + 1),
            });
        }
    }
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

/// Verifies every segment of the log in order, then checks the anchors against them.
pub fn verify_rotated(
    dir: &Path,
    stem: &str,
    key: Option<&VerifyingKey>,
    anchors: &[AnchorPoint],
) -> Result<Summary, AuditError> {
    let paths = segment_paths(dir, stem)?;
    if paths.is_empty() {
        return Err(AuditError::Refused {
            code: CODE_SEGMENT,
            reason: format!("no segments of `{stem}` in {}", dir.display()),
        });
    }
    let (summary, hashes) = verify_chain(&paths, key)?;
    check_anchors(&hashes, anchors)?;
    Ok(summary)
}

/// A log that rotates. `max_bytes` is the size at which a segment is closed; zero means no
/// limit. The signing key and the anchor, if any, carry over to each new segment.
pub struct RotatingLog {
    dir: PathBuf,
    stem: String,
    max_bytes: u64,
    policy: Policy,
    signing: Option<Signing>,
    current: AuditLog,
    segment: u64,
}

impl RotatingLog {
    /// Opens the log, or starts it. An existing log is verified in full first, and a broken
    /// segment or a missing one refuses the open.
    pub fn open(
        dir: impl Into<PathBuf>,
        stem: &str,
        max_bytes: u64,
        policy: Policy,
        signing: Option<Signing>,
    ) -> Result<Self, AuditError> {
        let dir = dir.into();
        if stem.is_empty() || !stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(AuditError::Io(format!(
                "log name `{stem}` must be letters, digits or '-'"
            )));
        }
        std::fs::create_dir_all(&dir)?;
        let paths = segment_paths(&dir, stem)?;
        let (segment, current) = if paths.is_empty() {
            let path = segment_path(&dir, stem, 1);
            (
                1,
                AuditLog::open_at(path, policy.clone(), signing.clone(), None)?,
            )
        } else {
            let verifying = signing.as_ref().map(|s| s.key.verifying_key());
            let (summary, _) = verify_chain(&paths, verifying.as_ref())?;
            let resume = crate::Resume::from_summary(&summary);
            let last = paths.len() as u64;
            let path = segment_path(&dir, stem, last);
            (
                last,
                AuditLog::open_at(path, policy.clone(), signing.clone(), Some(resume))?,
            )
        };
        Ok(RotatingLog {
            dir,
            stem: stem.to_string(),
            max_bytes,
            policy,
            signing,
            current,
            segment,
        })
    }

    /// Appends one entry, first closing the segment if it has reached the size limit.
    pub fn append(&mut self, entry: &Entry) -> Result<u64, AuditError> {
        if self.max_bytes > 0 && self.current.bytes() >= self.max_bytes && !self.current.is_empty()
        {
            self.rotate()?;
        }
        self.current.append(entry)
    }

    /// Sets the external anchor. It moves with the log to each new segment.
    pub fn set_anchor(&mut self, anchor: Box<dyn Anchor>) {
        self.current.set_anchor(anchor);
    }

    /// The number of the segment being written (1-based).
    pub fn segment(&self) -> u64 {
        self.segment
    }

    /// Records written so far, across all segments.
    pub fn len(&self) -> u64 {
        self.current.len()
    }

    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    fn rotate(&mut self) -> Result<(), AuditError> {
        let anchor = self.current.take_anchor();
        let resume = self.current.resume_point();
        self.segment += 1;
        let path = segment_path(&self.dir, &self.stem, self.segment);
        let mut next = AuditLog::open_at(
            path,
            self.policy.clone(),
            self.signing.clone(),
            Some(resume),
        )?;
        if let Some(anchor) = anchor {
            next.set_anchor(anchor);
        }
        self.current = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outcome;
    use ed25519_dalek::SigningKey;
    use serde_json::{json, Value};
    use std::sync::Arc;

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tpt-audit-seg-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(n: u64) -> Entry {
        Entry {
            wall_unix_ms: 1_700_000_000_000 + n,
            mono_ns: n,
            actor: "page".into(),
            command: format!("cmd_{n}"),
            capability: None,
            params: json!({ "n": n }),
            outcome: Outcome::Ok,
            duration_us: 1,
        }
    }

    fn signing(every: u64) -> Signing {
        Signing {
            key: Arc::new(SigningKey::from_bytes(&[5; 32])),
            every,
            every_secs: 0,
        }
    }

    fn verifying() -> VerifyingKey {
        SigningKey::from_bytes(&[5; 32]).verifying_key()
    }

    #[test]
    fn the_log_rotates_by_size_and_verifies_across_segments() {
        let dir = temp_dir("rotate");
        let mut log = RotatingLog::open(&dir, "audit", 400, Policy::default(), None).unwrap();
        for i in 0..20 {
            log.append(&entry(i)).unwrap();
        }
        assert!(
            log.segment() > 1,
            "the size limit should have closed a segment"
        );
        drop(log);
        let paths = segment_paths(&dir, "audit").unwrap();
        assert!(paths.len() > 1);
        let summary = verify_rotated(&dir, "audit", None, &[]).unwrap();
        assert_eq!(summary.records, 20);
    }

    #[test]
    fn a_new_segment_starts_with_the_previous_last_hash() {
        let dir = temp_dir("carry");
        let mut log = RotatingLog::open(&dir, "audit", 300, Policy::default(), None).unwrap();
        for i in 0..6 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        let first = std::fs::read_to_string(segment_path(&dir, "audit", 1)).unwrap();
        let second = std::fs::read_to_string(segment_path(&dir, "audit", 2)).unwrap();
        let last_of_first: Value = serde_json::from_str(first.lines().last().unwrap()).unwrap();
        let first_of_second: Value = serde_json::from_str(second.lines().next().unwrap()).unwrap();
        assert_eq!(first_of_second["prev"], last_of_first["hash"]);
        assert_eq!(
            first_of_second["seq"],
            last_of_first["seq"].as_u64().unwrap() + 1
        );
    }

    #[test]
    fn reopening_resumes_the_last_segment() {
        let dir = temp_dir("reopen");
        let mut log = RotatingLog::open(&dir, "audit", 300, Policy::default(), None).unwrap();
        for i in 0..5 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        let mut log = RotatingLog::open(&dir, "audit", 300, Policy::default(), None).unwrap();
        assert_eq!(log.len(), 5);
        log.append(&entry(5)).unwrap();
        drop(log);
        assert_eq!(verify_rotated(&dir, "audit", None, &[]).unwrap().records, 6);
    }

    #[test]
    fn a_missing_middle_segment_is_refused() {
        let dir = temp_dir("gap");
        let mut log = RotatingLog::open(&dir, "audit", 300, Policy::default(), None).unwrap();
        for i in 0..8 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        std::fs::remove_file(segment_path(&dir, "audit", 2)).unwrap();
        let err = segment_paths(&dir, "audit").unwrap_err();
        assert!(err.to_string().starts_with(CODE_SEGMENT));
    }

    #[test]
    fn a_deleted_first_segment_breaks_the_chain() {
        let dir = temp_dir("first");
        let mut log = RotatingLog::open(&dir, "audit", 300, Policy::default(), None).unwrap();
        for i in 0..8 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        std::fs::remove_file(segment_path(&dir, "audit", 1)).unwrap();
        // Renumber the rest so that numbering is contiguous again: the chain must still fail.
        let mut n = 2;
        while segment_path(&dir, "audit", n).exists() {
            std::fs::rename(
                segment_path(&dir, "audit", n),
                segment_path(&dir, "audit", n - 1),
            )
            .unwrap();
            n += 1;
        }
        assert!(verify_rotated(&dir, "audit", None, &[]).is_err());
    }

    #[test]
    fn signed_checkpoints_and_anchors_work_across_segments() {
        let dir = temp_dir("signed");
        let anchor_path = temp_dir("anchor-volume").join("anchors.txt");
        let mut log =
            RotatingLog::open(&dir, "audit", 500, Policy::default(), Some(signing(2))).unwrap();
        log.set_anchor(Box::new(crate::anchor::FileAnchor::new(&anchor_path)));
        for i in 0..10 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        let anchors = crate::anchor::read_file_anchors(&anchor_path).unwrap();
        assert!(!anchors.is_empty());
        let key = verifying();
        let summary = verify_rotated(&dir, "audit", Some(&key), &anchors).unwrap();
        assert!(summary.checkpoints >= 1);
    }

    #[test]
    fn a_truncated_log_fails_against_its_anchors() {
        let dir = temp_dir("truncate");
        let anchor_path = temp_dir("truncate-volume").join("anchors.txt");
        let mut log =
            RotatingLog::open(&dir, "audit", 0, Policy::default(), Some(signing(2))).unwrap();
        log.set_anchor(Box::new(crate::anchor::FileAnchor::new(&anchor_path)));
        for i in 0..6 {
            log.append(&entry(i)).unwrap();
        }
        drop(log);
        let anchors = crate::anchor::read_file_anchors(&anchor_path).unwrap();
        let last_anchor = anchors.last().unwrap().seq;
        // Cut the file back to just before the last anchored record.
        let path = segment_path(&dir, "audit", 1);
        let text = std::fs::read_to_string(&path).unwrap();
        let kept: Vec<&str> = text.lines().take(last_anchor as usize).collect();
        std::fs::write(&path, kept.join("\n") + "\n").unwrap();
        // Without the anchors, the shorter chain still verifies. With them, it does not.
        assert!(verify_rotated(&dir, "audit", Some(&verifying()), &[]).is_ok());
        let err = verify_rotated(&dir, "audit", Some(&verifying()), &anchors).unwrap_err();
        assert!(err
            .to_string()
            .starts_with(crate::anchor::CODE_ANCHOR_MISMATCH));
    }
}
