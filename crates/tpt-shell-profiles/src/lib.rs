// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Profiles and ephemeral sessions (spec §10.7, M6, the parts that need no OS-specific API).
//!
//! - [`Kind`]: default, named, or ephemeral.
//! - [`ProfileStore`]: create, list and delete named profiles under one base folder. Names are
//!   checked, and every path is built from a checked name, so a profile cannot reach outside
//!   the store.
//! - [`Ephemeral`]: a temporary session folder that is wiped when dropped or closed.
//! - [`wipe_dir`]: best-effort overwrite and delete. SSDs and journaling file systems can keep
//!   old copies; the documentation says so.
//! - [`usage`] and [`enforce_quota`]: the size of a profile and a cap on it.
//!
//! - [`export`]: encrypted export of a profile folder (ChaCha20-Poly1305, key from PBKDF2).
//!
//! Not here: the WebView2 and macOS and Linux mappings, which live with the backends.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Support: Full.
pub mod export;

/// Which storage a window uses.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The app's default profile.
    Default,
    /// A named profile that lasts until it is deleted.
    Named(String),
    /// A session that leaves nothing behind.
    Ephemeral,
}

/// Why a profile name was refused.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameError(pub String);

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for NameError {}

/// A profile name: 1-64 ASCII letters, digits, `-` or `_`. The reserved name `default` is not
/// allowed as a named profile, because it means the default profile.
///
/// Support: Full.
pub fn check_name(name: &str) -> Result<(), NameError> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(NameError(format!(
            "`{name}` must be 1-64 ASCII letters, digits, `-` or `_`"
        )));
    }
    if name.eq_ignore_ascii_case("default") {
        return Err(NameError(
            "`default` is the default profile, not a name".into(),
        ));
    }
    Ok(())
}

/// Folder for a profile under `base`. `Ephemeral` has no fixed folder; see [`Ephemeral`].
///
/// Support: Full.
pub fn profile_dir(base: &Path, kind: &Kind) -> Result<PathBuf, NameError> {
    match kind {
        Kind::Default => Ok(base.join("default")),
        Kind::Named(name) => {
            check_name(name)?;
            Ok(base.join("profiles").join(name))
        }
        Kind::Ephemeral => Err(NameError(
            "ephemeral sessions use their own temporary folder".into(),
        )),
    }
}

const ACTIVE_FILE: &str = "active.txt";

/// Named profiles under one base folder.
///
/// Support: Full.
pub struct ProfileStore {
    base: PathBuf,
}

impl ProfileStore {
    /// Support: Full.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        ProfileStore { base: base.into() }
    }

    /// Creates the profile's folder. Returns an error if it already exists.
    ///
    /// Support: Full.
    pub fn create(&self, name: &str) -> io::Result<PathBuf> {
        let dir = profile_dir(&self.base, &Kind::Named(name.into()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        if dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("profile `{name}` exists"),
            ));
        }
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Names of the existing profiles, sorted.
    ///
    /// Support: Full.
    pub fn list(&self) -> io::Result<Vec<String>> {
        let root = self.base.join("profiles");
        if !root.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    if check_name(name).is_ok() {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// The profile chosen with [`ProfileStore::switch`], or `None` for the default profile.
    /// The choice is kept in a file in the store, so it survives a restart.
    ///
    /// Support: Full.
    pub fn active(&self) -> io::Result<Option<String>> {
        match fs::read_to_string(self.base.join(ACTIVE_FILE)) {
            Ok(text) => {
                let name = text.trim();
                if name.is_empty() {
                    Ok(None)
                } else {
                    check_name(name)
                        .map(|_| Some(name.to_string()))
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.0))
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Makes `name` the active profile, or the default profile when `name` is `None`. The
    /// profile must exist. A running app keeps the folder it started with; the new choice is
    /// used by the next start, which reads it through [`ProfileStore::active`].
    ///
    /// Support: Full.
    pub fn switch(&self, name: Option<&str>) -> io::Result<Kind> {
        let kind = match name {
            None => Kind::Default,
            Some(name) => {
                let dir = profile_dir(&self.base, &Kind::Named(name.into()))
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
                if !dir.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("profile `{name}` does not exist"),
                    ));
                }
                Kind::Named(name.into())
            }
        };
        fs::create_dir_all(&self.base)?;
        let path = self.base.join(ACTIVE_FILE);
        let tmp = self.base.join(format!("{ACTIVE_FILE}.tmp"));
        let text = match &kind {
            Kind::Named(name) => name.as_str(),
            _ => "",
        };
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &path)?;
        Ok(kind)
    }

    /// Size of a named profile, in bytes.
    ///
    /// Support: Full.
    pub fn usage(&self, name: &str) -> io::Result<u64> {
        let dir = profile_dir(&self.base, &Kind::Named(name.into()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        usage(&dir)
    }

    /// Fails when the named profile is over `limit_bytes`.
    ///
    /// Support: Full.
    pub fn enforce_quota(&self, name: &str, limit_bytes: u64) -> io::Result<()> {
        let dir = profile_dir(&self.base, &Kind::Named(name.into()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        enforce_quota(&dir, limit_bytes)
    }

    /// Writes the named profile to `out` as an encrypted file (see [`export`]).
    ///
    /// Support: Full.
    pub fn export(&self, name: &str, out: &Path, passphrase: &str) -> io::Result<()> {
        let dir = profile_dir(&self.base, &Kind::Named(name.into()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        if !dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("profile `{name}` does not exist"),
            ));
        }
        export::export_dir(&dir, out, passphrase)
    }

    /// Deletes a named profile, overwriting its files first. Refuses names that do not pass
    /// [`check_name`], so the call cannot reach outside the store.
    ///
    /// Support: Full.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        let dir = profile_dir(&self.base, &Kind::Named(name.into()))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0))?;
        wipe_dir(&dir)?;
        if self.active()?.as_deref() == Some(name) {
            fs::remove_file(self.base.join(ACTIVE_FILE))?;
        }
        Ok(())
    }
}

/// A temporary session folder. It is wiped when dropped, and [`Ephemeral::wipe`] does the same
/// earlier, for example on window close or after idle time.
///
/// Support: Full.
pub struct Ephemeral {
    dir: PathBuf,
    wiped: bool,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Ephemeral {
    /// Creates a new empty session folder under `base`.
    ///
    /// Support: Full.
    pub fn create(base: &Path) -> io::Result<Self> {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = base
            .join("ephemeral")
            .join(format!("s{}-{n}", std::process::id()));
        fs::create_dir_all(&dir)?;
        Ok(Ephemeral { dir, wiped: false })
    }

    /// Support: Full.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Overwrites and deletes everything in the session folder now.
    ///
    /// Support: Full.
    pub fn wipe(&mut self) -> io::Result<()> {
        if self.wiped {
            return Ok(());
        }
        self.wiped = true;
        wipe_dir(&self.dir)
    }
}

impl Drop for Ephemeral {
    fn drop(&mut self) {
        if !self.wiped {
            let _ = wipe_dir(&self.dir);
        }
    }
}

/// Overwrites every file under `dir` with zeros, then removes the tree.
///
/// This is best effort. Copy-on-write file systems, SSD wear levelling, journals and backups
/// can keep old data, so this does not guarantee erasure. Use full-disk encryption for that.
///
/// Support: Partial. Best effort, as the doc says: copy-on-write file systems, SSD wear levelling,
/// journals and backups can keep old data, so erasure is not guaranteed.
pub fn wipe_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    overwrite_files(dir)?;
    fs::remove_dir_all(dir)
}

fn overwrite_files(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            overwrite_files(&path)?;
        } else if file_type.is_file() {
            let len = fs::metadata(&path)?.len();
            let mut file = fs::OpenOptions::new().write(true).open(&path)?;
            let zeros = vec![0u8; 64 * 1024];
            let mut left = len;
            while left > 0 {
                let n = left.min(zeros.len() as u64) as usize;
                file.write_all(&zeros[..n])?;
                left -= n as u64;
            }
            file.sync_all()?;
        }
    }
    Ok(())
}

/// Total size of the files under `dir`, in bytes.
///
/// Support: Full.
pub fn usage(dir: &Path) -> io::Result<u64> {
    let mut total = 0;
    if !dir.exists() {
        return Ok(0);
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            total += usage(&entry.path())?;
        } else if file_type.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

/// Fails when `dir` is over `limit_bytes`.
///
/// Support: Full.
pub fn enforce_quota(dir: &Path, limit_bytes: u64) -> io::Result<()> {
    let used = usage(dir)?;
    if used > limit_bytes {
        Err(io::Error::other(format!(
            "profile uses {used} bytes, quota is {limit_bytes}"
        )))
    } else {
        Ok(())
    }
}

/// When an ephemeral session should be wiped without waiting for close.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdlePolicy {
    /// Wipe after this much time without activity. `None` means never.
    pub after: Option<Duration>,
}

impl IdlePolicy {
    /// Support: Full.
    pub fn should_wipe(&self, last_activity: Instant, now: Instant) -> bool {
        match self.after {
            Some(limit) => now.saturating_duration_since(last_activity) >= limit,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("tpt-profiles-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_are_checked_and_default_is_reserved() {
        assert!(check_name("work-1").is_ok());
        for bad in [
            "",
            "a b",
            "../x",
            "x/y",
            "default",
            "Default",
            &"a".repeat(65),
        ] {
            assert!(check_name(bad).is_err(), "accepted `{bad}`");
        }
    }

    #[test]
    fn profile_folders_are_under_the_base() {
        let base = Path::new("base");
        assert_eq!(
            profile_dir(base, &Kind::Default).unwrap(),
            base.join("default")
        );
        assert_eq!(
            profile_dir(base, &Kind::Named("work".into())).unwrap(),
            base.join("profiles").join("work")
        );
        assert!(profile_dir(base, &Kind::Named("../evil".into())).is_err());
        assert!(profile_dir(base, &Kind::Ephemeral).is_err());
    }

    #[test]
    fn create_list_and_delete_named_profiles() {
        let base = scratch("store");
        let store = ProfileStore::new(&base);
        store.create("work").unwrap();
        store.create("home").unwrap();
        assert!(store.create("work").is_err(), "a second create must fail");
        assert_eq!(store.list().unwrap(), vec!["home", "work"]);
        store.delete("home").unwrap();
        assert_eq!(store.list().unwrap(), vec!["work"]);
        assert!(store.delete("../work").is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn switch_records_the_active_profile_and_delete_clears_it() {
        let base = scratch("switch");
        let store = ProfileStore::new(&base);
        assert_eq!(store.active().unwrap(), None);
        assert!(store.switch(Some("work")).is_err(), "a missing profile");
        store.create("work").unwrap();
        assert_eq!(
            store.switch(Some("work")).unwrap(),
            Kind::Named("work".into())
        );
        assert_eq!(store.active().unwrap().as_deref(), Some("work"));
        // A new store over the same folder reads the same choice, as a restart would.
        assert_eq!(
            ProfileStore::new(&base).active().unwrap().as_deref(),
            Some("work")
        );
        assert_eq!(store.switch(None).unwrap(), Kind::Default);
        assert_eq!(store.active().unwrap(), None);
        store.switch(Some("work")).unwrap();
        store.delete("work").unwrap();
        assert_eq!(store.active().unwrap(), None);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_store_quota_applies_to_one_named_profile() {
        let base = scratch("store-quota");
        let store = ProfileStore::new(&base);
        let dir = store.create("big").unwrap();
        fs::write(dir.join("cache.bin"), vec![0u8; 2048]).unwrap();
        assert_eq!(store.usage("big").unwrap(), 2048);
        assert!(store.enforce_quota("big", 4096).is_ok());
        assert!(store.enforce_quota("big", 1024).is_err());
        assert!(store.usage("../big").is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn profiles_are_isolated_on_disk() {
        let base = scratch("isolation");
        let store = ProfileStore::new(&base);
        let a = store.create("a").unwrap();
        let b = store.create("b").unwrap();
        fs::write(a.join("secret.txt"), "only in a").unwrap();
        assert!(!b.join("secret.txt").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn deleting_overwrites_file_contents_before_removal() {
        let base = scratch("erase");
        let store = ProfileStore::new(&base);
        let dir = store.create("erase-me").unwrap();
        let file = dir.join("cookies.db");
        fs::write(&file, "sensitive bytes").unwrap();
        // Check the overwrite step on its own, before the tree is removed.
        overwrite_files(&dir).unwrap();
        let after = fs::read(&file).unwrap();
        assert_eq!(after.len(), "sensitive bytes".len());
        assert!(after.iter().all(|b| *b == 0));
        store.delete("erase-me").unwrap();
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ephemeral_sessions_leave_no_residue() {
        let base = scratch("ephemeral");
        let path = {
            let session = Ephemeral::create(&base).unwrap();
            fs::write(session.path().join("page-state.json"), "{}").unwrap();
            session.path().to_path_buf()
        };
        assert!(!path.exists(), "dropping the session must wipe it");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn explicit_wipe_removes_the_session_and_drop_does_not_repeat_it() {
        let base = scratch("wipe");
        let mut session = Ephemeral::create(&base).unwrap();
        fs::write(session.path().join("x"), "1").unwrap();
        let path = session.path().to_path_buf();
        session.wipe().unwrap();
        assert!(!path.exists());
        session.wipe().unwrap();
        drop(session);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn two_ephemeral_sessions_do_not_share_a_folder() {
        let base = scratch("two");
        let a = Ephemeral::create(&base).unwrap();
        let b = Ephemeral::create(&base).unwrap();
        assert_ne!(a.path(), b.path());
        drop((a, b));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn quota_counts_nested_files() {
        let base = scratch("quota");
        fs::create_dir_all(base.join("deep/er")).unwrap();
        fs::write(base.join("deep/er/a.bin"), vec![0u8; 100]).unwrap();
        fs::write(base.join("b.bin"), vec![0u8; 50]).unwrap();
        assert_eq!(usage(&base).unwrap(), 150);
        assert!(enforce_quota(&base, 150).is_ok());
        assert!(enforce_quota(&base, 149).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn idle_policy_wipes_only_after_the_limit() {
        let policy = IdlePolicy {
            after: Some(Duration::from_secs(60)),
        };
        let start = Instant::now();
        assert!(!policy.should_wipe(start, start + Duration::from_secs(59)));
        assert!(policy.should_wipe(start, start + Duration::from_secs(60)));
        let never = IdlePolicy { after: None };
        assert!(!never.should_wipe(start, start + Duration::from_secs(3600)));
    }
}
