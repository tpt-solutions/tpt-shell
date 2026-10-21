// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! External anchors (spec §10.3). Each signed checkpoint is also written to a second sink, so
//! that truncating the primary log is detectable: the anchored record must still be there with
//! the same hash.
//!
//! Sinks: [`FileAnchor`] (a file on another volume), and [`SyslogAnchor`] (an RFC 5424 line
//! written to any writer the caller opens, such as a local syslog socket). An HTTP anchor is
//! not built. It would open a network socket, so it is left to a feature that is off by default.
//!
//! Limit: records written after the last anchored checkpoint are not covered. Anchor often, or
//! anchor each segment.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::AuditError;

/// No anchor could be written. Fix: check the anchor sink, such as its volume and permissions.
pub const CODE_ANCHOR_WRITE: &str = "TS0102";
/// An anchor is missing from the log, or the log does not hold the anchored record. The log
/// was truncated or replaced.
pub const CODE_ANCHOR_MISMATCH: &str = "TS0103";

/// One anchored checkpoint: the sequence number and hash of the checkpoint record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorPoint {
    pub seq: u64,
    pub hash: String,
    pub wall_unix_ms: u64,
}

/// A second sink for checkpoint hashes. Called once per checkpoint, after it is on disk.
pub trait Anchor: Send {
    fn publish(&mut self, point: &AnchorPoint) -> Result<(), AuditError>;
}

fn write_error(reason: impl Into<String>) -> AuditError {
    AuditError::Refused {
        code: CODE_ANCHOR_WRITE,
        reason: reason.into(),
    }
}

/// Appends `seq wall_unix_ms hash` lines to a file that should live on another volume.
pub struct FileAnchor {
    path: PathBuf,
}

impl FileAnchor {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileAnchor { path: path.into() }
    }
}

impl Anchor for FileAnchor {
    fn publish(&mut self, point: &AnchorPoint) -> Result<(), AuditError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| {
                write_error(format!(
                    "cannot open the anchor file {}: {e}",
                    self.path.display()
                ))
            })?;
        let line = format!("{} {} {}\n", point.seq, point.wall_unix_ms, point.hash);
        file.write_all(line.as_bytes())
            .and_then(|()| file.sync_data())
            .map_err(|e| write_error(format!("cannot write the anchor: {e}")))
    }
}

/// Reads the anchors a [`FileAnchor`] wrote.
pub fn read_file_anchors(path: impl AsRef<Path>) -> Result<Vec<AnchorPoint>, AuditError> {
    let reader = BufReader::new(std::fs::File::open(path.as_ref())?);
    let mut points = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line?;
        let mismatch = |reason: &str| AuditError::Refused {
            code: CODE_ANCHOR_MISMATCH,
            reason: format!("anchor file line {}: {reason}", i + 1),
        };
        let mut parts = line.split(' ');
        let seq = parts
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| mismatch("no sequence number"))?;
        let wall_unix_ms = parts
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| mismatch("no wall time"))?;
        let hash = parts.next().ok_or_else(|| mismatch("no hash"))?;
        if parts.next().is_some() || hash.len() != 64 {
            return Err(mismatch("malformed"));
        }
        points.push(AnchorPoint {
            seq,
            hash: hash.to_string(),
            wall_unix_ms,
        });
    }
    Ok(points)
}

/// Formats one anchor as an RFC 5424 message. The timestamp field is the NILVALUE `-`, and the
/// wall time is carried in the message text. `host` has spaces replaced, so it stays one field.
pub fn syslog_line(point: &AnchorPoint, host: &str) -> String {
    let host: String = host
        .chars()
        .map(|c| if c.is_ascii_graphic() { c } else { '-' })
        .collect();
    let host = if host.is_empty() {
        "-".to_string()
    } else {
        host
    };
    format!(
        "<134>1 - {host} tpt-shell - - - audit-checkpoint seq={} wall_unix_ms={} hash={}",
        point.seq, point.wall_unix_ms, point.hash
    )
}

/// Writes one RFC 5424 line per checkpoint to any writer the caller opened.
pub struct SyslogAnchor<W: Write + Send> {
    out: W,
    host: String,
}

impl<W: Write + Send> SyslogAnchor<W> {
    pub fn new(out: W, host: impl Into<String>) -> Self {
        SyslogAnchor {
            out,
            host: host.into(),
        }
    }
}

impl<W: Write + Send> Anchor for SyslogAnchor<W> {
    fn publish(&mut self, point: &AnchorPoint) -> Result<(), AuditError> {
        let line = syslog_line(point, &self.host);
        writeln!(self.out, "{line}")
            .and_then(|()| self.out.flush())
            .map_err(|e| write_error(format!("cannot write to the syslog sink: {e}")))
    }
}

/// Checks that every anchored record is in the chain with the anchored hash. `hashes[i]` is the
/// hash of record `i`. A log shorter than an anchor fails, which is how truncation shows.
pub(crate) fn check_anchors(hashes: &[String], anchors: &[AnchorPoint]) -> Result<(), AuditError> {
    for point in anchors {
        let idx = usize::try_from(point.seq).unwrap_or(usize::MAX);
        match hashes.get(idx) {
            None => {
                return Err(AuditError::Refused {
                    code: CODE_ANCHOR_MISMATCH,
                    reason: format!(
                        "the log ends before anchored record {}: it was truncated or replaced",
                        point.seq
                    ),
                })
            }
            Some(hash) if *hash != point.hash => {
                return Err(AuditError::Refused {
                    code: CODE_ANCHOR_MISMATCH,
                    reason: format!(
                        "record {} does not match its anchor: the log was rewritten",
                        point.seq
                    ),
                })
            }
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(seq: u64) -> AnchorPoint {
        AnchorPoint {
            seq,
            hash: format!("{:064x}", seq + 1),
            wall_unix_ms: 1_700_000_000_000 + seq,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tpt-audit-anchor-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn file_anchors_round_trip() {
        let path = temp_path("anchors.txt");
        let mut anchor = FileAnchor::new(&path);
        anchor.publish(&point(4)).unwrap();
        anchor.publish(&point(9)).unwrap();
        assert_eq!(read_file_anchors(&path).unwrap(), vec![point(4), point(9)]);
    }

    #[test]
    fn a_malformed_anchor_line_is_refused() {
        let path = temp_path("bad-anchor.txt");
        std::fs::write(&path, "4 1700 short\n").unwrap();
        let err = read_file_anchors(&path).unwrap_err();
        assert!(err.to_string().starts_with(CODE_ANCHOR_MISMATCH));
    }

    #[test]
    fn syslog_lines_are_single_rfc5424_messages() {
        let line = syslog_line(&point(2), "host one");
        assert_eq!(
            line,
            format!(
                "<134>1 - host-one tpt-shell - - - audit-checkpoint seq=2 wall_unix_ms={} hash={}",
                1_700_000_000_002u64,
                point(2).hash
            )
        );
        assert!(!line.contains('\n'));
    }

    #[test]
    fn syslog_anchor_writes_one_line_per_checkpoint() {
        let mut out: Vec<u8> = Vec::new();
        {
            let mut anchor = SyslogAnchor::new(&mut out, "h");
            anchor.publish(&point(1)).unwrap();
            anchor.publish(&point(3)).unwrap();
        }
        assert_eq!(String::from_utf8(out).unwrap().lines().count(), 2);
    }

    #[test]
    fn anchors_detect_a_short_or_rewritten_log() {
        let hashes: Vec<String> = (0..5).map(|i| format!("{:064x}", i + 1)).collect();
        assert!(check_anchors(&hashes, &[point(4)]).is_ok());
        let short = check_anchors(&hashes[..3], &[point(4)]).unwrap_err();
        assert!(short.to_string().contains("ends before anchored record 4"));
        let mut rewritten = hashes.clone();
        rewritten[4] = "f".repeat(64);
        assert!(check_anchors(&rewritten, &[point(4)]).is_err());
    }
}
