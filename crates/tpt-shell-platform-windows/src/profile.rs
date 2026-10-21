// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Maps a tpt-shell profile to the WebView2 user-data folder that holds its browser state
//! (spec 10.7). WebView2 keeps one folder per environment, so each profile needs its own
//! environment on this folder.
//!
//! - The default profile lives in `<base>\webview2`. That is the folder
//!   [`crate::webview2::default_user_data_dir`] already uses, so existing data stays where it is.
//! - A named profile lives in `<base>\profiles\<name>` (see `tpt_shell_profiles::profile_dir`).
//! - An ephemeral session gets a fresh folder under `<base>\ephemeral`. Dropping the returned
//!   [`UserData`] wipes it.

use std::io;
use std::path::{Path, PathBuf};

use tpt_shell_profiles::{profile_dir, Ephemeral, Kind};

/// Folder name for the default profile under the base folder.
const DEFAULT_FOLDER: &str = "webview2";

/// The user-data folder for one profile. An ephemeral session owns its folder and wipes it on
/// drop.
pub enum UserData {
    /// A folder that stays after the app closes.
    Fixed(PathBuf),
    /// A temporary session folder, wiped when this value is dropped.
    Ephemeral(Ephemeral),
}

impl UserData {
    /// The folder WebView2 should use.
    pub fn path(&self) -> &Path {
        match self {
            UserData::Fixed(path) => path,
            UserData::Ephemeral(session) => session.path(),
        }
    }
}

/// Chooses the user-data folder for `kind` under `base`. Creates the ephemeral folder; fixed
/// folders are created by WebView2 on first use.
pub fn user_data_for(base: &Path, kind: &Kind) -> io::Result<UserData> {
    match kind {
        Kind::Default => Ok(UserData::Fixed(base.join(DEFAULT_FOLDER))),
        Kind::Named(_) => profile_dir(base, kind)
            .map(UserData::Fixed)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.0)),
        Kind::Ephemeral => Ephemeral::create(base).map(UserData::Ephemeral),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        std::env::temp_dir().join(format!("tpt-profile-map-{}", std::process::id()))
    }

    #[test]
    fn the_default_profile_keeps_the_existing_webview2_folder() {
        let root = Path::new(r"C:\Users\me\AppData\Local\tpt-shell\app");
        let data = user_data_for(root, &Kind::Default).unwrap();
        assert_eq!(data.path(), root.join("webview2"));
    }

    #[test]
    fn a_named_profile_gets_its_own_folder_and_bad_names_are_refused() {
        let root = Path::new(r"C:\base");
        let data = user_data_for(root, &Kind::Named("work".into())).unwrap();
        assert_eq!(data.path(), root.join("profiles").join("work"));
        assert!(user_data_for(root, &Kind::Named("../evil".into())).is_err());
    }

    #[test]
    fn an_ephemeral_session_gets_a_fresh_folder_that_is_wiped_on_drop() {
        let root = base();
        std::fs::create_dir_all(&root).unwrap();
        let first = user_data_for(&root, &Kind::Ephemeral).unwrap();
        let second = user_data_for(&root, &Kind::Ephemeral).unwrap();
        assert_ne!(first.path(), second.path());
        assert!(first.path().is_dir());
        let kept = first.path().to_path_buf();
        std::fs::write(kept.join("cookie"), b"x").unwrap();
        drop(first);
        assert!(!kept.exists());
        drop(second);
        let _ = std::fs::remove_dir_all(&root);
    }
}
