// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Single instance without a socket (todo M5 decision: no UNIX domain socket is opened).
//!
//! The first instance holds an exclusive advisory lock on `<dir>/<app-id>.lock`. A later
//! instance finds the lock taken, writes its arguments as one file in `<dir>/<app-id>.inbox/`,
//! and exits. The first instance collects those files with [`InstanceGuard::take_forwarded`].
//!
//! The lock is released by the operating system when the holder exits, even after a crash, so
//! a stale lock file never blocks a new start. Message files are written under a temporary
//! name and renamed, so a reader never sees half a message.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of [`claim`].
#[derive(Debug)]
pub enum Claim {
    /// This process is the running instance. Keep the guard alive for the app's lifetime.
    Primary(InstanceGuard),
    /// Another instance is running. The arguments were handed to it.
    Forwarded,
}

/// Holds the lock. Dropping it lets the next instance become primary.
#[derive(Debug)]
pub struct InstanceGuard {
    _lock: File,
    inbox: PathBuf,
}

/// Claims the single-instance lock for `app_id`, or forwards `args` to the running instance.
///
/// `dir` must exist or be creatable; use a per-user directory such as `$XDG_RUNTIME_DIR`.
/// `app_id` must be ASCII letters, digits, `-`, `_` or `.`; anything else is refused, since it
/// is used in file names.
pub fn claim(dir: &Path, app_id: &str, args: &[String]) -> io::Result<Claim> {
    validate_app_id(app_id)?;
    fs::create_dir_all(dir)?;
    let lock_path = dir.join(format!("{app_id}.lock"));
    let inbox = dir.join(format!("{app_id}.inbox"));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    match lock.try_lock() {
        Ok(()) => {
            fs::create_dir_all(&inbox)?;
            Ok(Claim::Primary(InstanceGuard { _lock: lock, inbox }))
        }
        Err(fs::TryLockError::WouldBlock) => {
            fs::create_dir_all(&inbox)?;
            write_message(&inbox, args)?;
            Ok(Claim::Forwarded)
        }
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

impl InstanceGuard {
    /// Returns the argument lists forwarded by later instances, oldest first, and deletes them.
    /// A message that cannot be read is skipped and left in place for inspection.
    pub fn take_forwarded(&self) -> io::Result<Vec<Vec<String>>> {
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.inbox)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "msg"))
            .collect();
        paths.sort();
        let mut out = Vec::new();
        for path in paths {
            let Ok(bytes) = fs::read(&path) else { continue };
            let Some(args) = decode_args(&bytes) else {
                continue;
            };
            fs::remove_file(&path)?;
            out.push(args);
        }
        Ok(out)
    }
}

fn validate_app_id(app_id: &str) -> io::Result<()> {
    let ok = !app_id.is_empty()
        && app_id.len() <= 128
        && app_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !app_id.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "app id must be 1-128 ASCII letters, digits, '-', '_' or '.', and not start with '.'",
        ))
    }
}

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Writes `args` as a length-prefixed record: each argument is its byte length in decimal, a
/// newline, then the bytes. Lengths avoid any escaping, so arguments may hold newlines.
fn write_message(inbox: &Path, args: &[String]) -> io::Result<()> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stem = format!("{nanos:020}-{}-{seq}", std::process::id());
    let tmp = inbox.join(format!("{stem}.tmp"));
    let msg = inbox.join(format!("{stem}.msg"));
    let mut body = Vec::new();
    for arg in args {
        body.extend_from_slice(format!("{}\n", arg.len()).as_bytes());
        body.extend_from_slice(arg.as_bytes());
    }
    let mut file = File::create(&tmp)?;
    file.write_all(&body)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, &msg)
}

/// Parses a record written by [`write_message`]. `None` for any malformed record.
fn decode_args(mut bytes: &[u8]) -> Option<Vec<String>> {
    let mut args = Vec::new();
    while !bytes.is_empty() {
        let newline = bytes.iter().position(|&b| b == b'\n')?;
        let len: usize = std::str::from_utf8(&bytes[..newline]).ok()?.parse().ok()?;
        bytes = &bytes[newline + 1..];
        if bytes.len() < len {
            return None;
        }
        let arg = std::str::from_utf8(&bytes[..len]).ok()?;
        args.push(arg.to_string());
        bytes = &bytes[len..];
    }
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory under the system temp dir. Tests never touch the user's files.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tpt-shell-single-{name}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn first_claim_is_primary_and_second_forwards_its_arguments() {
        let dir = temp_dir("forward");
        let first = match claim(&dir, "demo.app", &[]).unwrap() {
            Claim::Primary(guard) => guard,
            Claim::Forwarded => panic!("the first claim must be primary"),
        };
        let args = vec![
            "demo".to_string(),
            "tpt://open?x=1\nsecond line".to_string(),
        ];
        match claim(&dir, "demo.app", &args).unwrap() {
            Claim::Forwarded => {}
            Claim::Primary(_) => panic!("a second claim must not be primary"),
        }
        let forwarded = first.take_forwarded().unwrap();
        assert_eq!(forwarded, vec![args]);
        assert!(
            first.take_forwarded().unwrap().is_empty(),
            "messages are removed once read"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropping_the_primary_lets_the_next_claim_become_primary() {
        let dir = temp_dir("release");
        let first = claim(&dir, "demo", &[]).unwrap();
        drop(first);
        assert!(matches!(
            claim(&dir, "demo", &[]).unwrap(),
            Claim::Primary(_)
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_ids_that_could_escape_the_directory_are_refused() {
        let dir = temp_dir("ids");
        for bad in ["", "../x", "a/b", ".hidden", "sp ace"] {
            assert!(claim(&dir, bad, &[]).is_err(), "{bad:?} must be refused");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_records_are_rejected() {
        assert_eq!(decode_args(b"3\nabc"), Some(vec!["abc".to_string()]));
        assert_eq!(
            decode_args(b"5\nabc"),
            None,
            "a length longer than the data"
        );
        assert_eq!(
            decode_args(b"x\nabc"),
            None,
            "a length that is not a number"
        );
        assert_eq!(decode_args(b"3abc"), None, "a missing newline");
        assert_eq!(decode_args(b""), Some(vec![]));
    }
}
