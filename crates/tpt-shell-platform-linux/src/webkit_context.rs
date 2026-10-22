// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Which WebKit context the Linux adapters use, and where its data lives. Pure rules, tested on
//! every platform; the adapters in [`crate::gtk3`] and [`crate::gtk4`] apply them.
//!
//! By default each backend makes its own `WebKitWebContext` (and, on GTK 4, its own
//! `WebKitNetworkSession`) and releases it when the backend is dropped, before the process can
//! exit. WebKit's process-wide default context is finalised by an `atexit` handler, and in CI
//! that handler aborted the process (SIGABRT, exit 134) on both adapters; an owned context does
//! not. `TPT_WEBKIT_CONTEXT=default` returns to the default context as an escape hatch.
//!
//! Storage: **a context we own stores nothing on disk unless a data directory is given.** Cookies,
//! local storage and caches are then ephemeral: they are gone when the backend is dropped. That is
//! different from the default context, which persists them, so it is documented as Partial in
//! `docs/src/platform-notes.md`. A data directory comes from `TPT_WEBKIT_DATA_DIR` or from
//! `with_data_dir` on the backend.

use std::path::{Component, Path, PathBuf};

/// The environment variable that chooses the context.
///
/// Support: Full.
pub const ENV_CONTEXT: &str = "TPT_WEBKIT_CONTEXT";

/// The environment variable that names the data directory.
///
/// Support: Full.
pub const ENV_DATA_DIR: &str = "TPT_WEBKIT_DATA_DIR";

/// Which context the backend uses.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMode {
    /// A context made and released by the backend (the default).
    Own,
    /// WebKit's process-wide default context. Persistent, but its exit-time teardown aborted in
    /// CI, so it is an escape hatch only.
    Default,
}

/// Parses `TPT_WEBKIT_CONTEXT`. Unset or empty is `Own`. Only `own` and `default` are accepted,
/// so a typo is an error and not a silent choice.
///
/// Support: Full.
pub fn parse_mode(value: Option<&str>) -> Result<ContextMode, String> {
    match value.map(str::trim) {
        None | Some("") | Some("own") => Ok(ContextMode::Own),
        Some("default") => Ok(ContextMode::Default),
        Some(other) => Err(format!(
            "{ENV_CONTEXT} must be `own` or `default`, not `{other}`; unset it for the default"
        )),
    }
}

/// Where the owned context keeps its data.
///
/// Support: Partial. Ephemeral unless a data directory is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Nothing is written to disk.
    Ephemeral,
    /// Data under `data`, caches under `cache`; both were created with mode 0700.
    Persistent { data: PathBuf, cache: PathBuf },
}

/// The context settings of one backend.
///
/// Support: Partial. Ephemeral unless a data directory is given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextConfig {
    pub mode: ContextMode,
    pub storage: Storage,
}

impl ContextConfig {
    /// The settings the environment asks for. A bad value, or a data directory that fails
    /// [`prepare_storage`], is an error.
    ///
    /// Support: Partial. Ephemeral unless a data directory is given.
    pub fn from_env() -> Result<ContextConfig, String> {
        let mode = parse_mode(std::env::var(ENV_CONTEXT).ok().as_deref())?;
        let dir = std::env::var_os(ENV_DATA_DIR).filter(|v| !v.is_empty());
        let storage = match (&dir, mode) {
            // The default context keeps its own data; a directory has nowhere to go.
            (Some(_), ContextMode::Default) => {
                return Err(format!(
                    "{ENV_DATA_DIR} needs the owned context, but {ENV_CONTEXT} is `default`"
                ))
            }
            (Some(dir), ContextMode::Own) => prepare_storage(Some(Path::new(dir)))?,
            (None, _) => Storage::Ephemeral,
        };
        Ok(ContextConfig { mode, storage })
    }

    /// Switches to persistent storage in `dir`. Refused with the default context.
    ///
    /// Support: Partial. Ephemeral unless a data directory is given.
    pub fn with_data_dir(&mut self, dir: &Path) -> Result<(), String> {
        if self.mode == ContextMode::Default {
            return Err(format!(
                "a data directory needs the owned context, but {ENV_CONTEXT} is `default`"
            ));
        }
        self.storage = prepare_storage(Some(dir))?;
        Ok(())
    }
}

/// Checks a data directory without touching the disk: it must be absolute, hold no `..`
/// component and no NUL byte.
///
/// Support: Full.
pub fn validate_data_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_absolute() {
        return Err(format!(
            "the data directory {} is not absolute",
            dir.display()
        ));
    }
    if dir.components().any(|c| c == Component::ParentDir) {
        return Err(format!(
            "the data directory {} contains `..`",
            dir.display()
        ));
    }
    if dir.to_string_lossy().contains('\0') {
        return Err("the data directory holds a NUL byte".into());
    }
    Ok(())
}

/// Validates `dir`, creates it and its `data` and `cache` folders with mode 0700, and returns the
/// persistent storage. `None` is ephemeral.
///
/// Support: Partial. Linux (and other Unix) permissions; elsewhere the folders get the default
/// mode.
pub fn prepare_storage(dir: Option<&Path>) -> Result<Storage, String> {
    let Some(dir) = dir else {
        return Ok(Storage::Ephemeral);
    };
    validate_data_dir(dir)?;
    let data = dir.join("data");
    let cache = dir.join("cache");
    for folder in [dir, data.as_path(), cache.as_path()] {
        create_private(folder)?;
    }
    Ok(Storage::Persistent { data, cache })
}

/// Creates a folder (and its parents) readable by the owner only.
fn create_private(folder: &Path) -> Result<(), String> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    // A folder that already existed keeps its mode, so tighten it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot restrict {}: {e}", folder.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-webkit-ctx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_owned_context_is_the_default_and_typos_are_errors() {
        assert_eq!(parse_mode(None), Ok(ContextMode::Own));
        assert_eq!(parse_mode(Some("")), Ok(ContextMode::Own));
        assert_eq!(parse_mode(Some("own")), Ok(ContextMode::Own));
        assert_eq!(parse_mode(Some(" default ")), Ok(ContextMode::Default));
        let err = parse_mode(Some("defualt")).unwrap_err();
        assert!(err.contains("TPT_WEBKIT_CONTEXT"), "{err}");
        assert!(parse_mode(Some("1")).is_err());
    }

    #[test]
    fn no_directory_means_ephemeral_storage() {
        assert_eq!(prepare_storage(None), Ok(Storage::Ephemeral));
    }

    #[test]
    fn data_directories_must_be_absolute_and_free_of_parent_components() {
        assert!(validate_data_dir(Path::new("relative/dir")).is_err());
        let base = std::env::temp_dir();
        assert!(validate_data_dir(&base.join("a").join("..").join("b")).is_err());
        assert!(validate_data_dir(&base.join("a")).is_ok());
    }

    #[test]
    fn a_data_directory_gives_persistent_storage_in_private_folders() {
        let dir = scratch("persist");
        let storage = prepare_storage(Some(&dir)).expect("the folders are created");
        match storage {
            Storage::Persistent { data, cache } => {
                assert!(data.is_dir() && cache.is_dir());
                assert_eq!(data, dir.join("data"));
                assert_eq!(cache, dir.join("cache"));
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
                    assert_eq!(mode, 0o700);
                }
            }
            Storage::Ephemeral => panic!("a directory must give persistent storage"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_directory_creates_nothing() {
        let base = std::env::temp_dir();
        let bad = base.join("tpt-webkit-ctx-bad").join("..").join("x");
        assert!(prepare_storage(Some(&bad)).is_err());
        assert!(!base.join("tpt-webkit-ctx-bad").exists());
    }

    #[test]
    fn a_data_directory_is_refused_with_the_default_context() {
        let mut config = ContextConfig {
            mode: ContextMode::Default,
            storage: Storage::Ephemeral,
        };
        let dir = scratch("refused");
        assert!(config.with_data_dir(&dir).is_err());
        assert!(!dir.exists(), "nothing is created for a refused directory");
        let mut own = ContextConfig {
            mode: ContextMode::Own,
            storage: Storage::Ephemeral,
        };
        own.with_data_dir(&dir).expect("the owned context takes it");
        assert!(matches!(own.storage, Storage::Persistent { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
