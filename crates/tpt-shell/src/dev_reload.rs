// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Dev reload. A debug build of an app whose environment names a folder in `TPT_DEV_DIR` serves
//! its pages from that folder, and reloads its windows when a file in it changes. The window
//! loads nothing over the network: the folder is read through the same app-origin requests the
//! embedded assets use, and the change check is a timer on the UI thread. Release builds ignore
//! the variable, so a shipped binary never serves pages from disk.

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tpt_shell_core::backend::Asset;

/// The environment variable that names the app's web folder.
pub const ENV: &str = "TPT_DEV_DIR";

/// How often the UI thread checks the folder for a change.
pub const PERIOD: std::time::Duration = std::time::Duration::from_millis(250);

/// Most files one check reads. A folder past this is too large to watch, so the extra files are
/// not counted. That can miss a change in them; it does not cause a false reload.
const MAX_FILES: usize = 4096;

/// Deepest folder the check descends into.
const MAX_DEPTH: usize = 16;

/// The folder named by `TPT_DEV_DIR`, for a debug build. `None` when the variable is unset, when
/// the build is a release build (which says so on stderr), or when the folder does not exist.
pub fn dev_dir() -> Option<PathBuf> {
    dev_dir_from(std::env::var_os(ENV), cfg!(debug_assertions))
}

/// The rule behind [`dev_dir`], with its inputs given, so it can be tested.
fn dev_dir_from(value: Option<OsString>, debug_build: bool) -> Option<PathBuf> {
    let value = value?;
    if !debug_build {
        eprintln!("tpt-shell: {ENV} is ignored in a release build");
        return None;
    }
    let path = PathBuf::from(value);
    if !path.is_dir() {
        eprintln!(
            "tpt-shell: {ENV} is not a folder, so pages are served from the build: {}",
            path.display()
        );
        return None;
    }
    Some(path)
}

/// The asset for `path` (a path under the app origin, such as `js/app.js`), read from `root`.
/// `None` for a file that is missing, a folder, or any path that would leave `root`: a `..` or
/// `.` segment, a backslash, a colon (a drive or a stream name), an empty segment, or a link that
/// resolves outside the folder.
pub fn disk_asset(root: &Path, path: &str) -> Option<Asset> {
    let relative = safe_relative(path)?;
    let real_root = root.canonicalize().ok()?;
    let real = root.join(relative).canonicalize().ok()?;
    if !real.starts_with(&real_root) || !real.is_file() {
        return None;
    }
    let body = fs::read(&real).ok()?;
    Some(Asset {
        content_type: content_type(path).to_string(),
        body,
    })
}

/// The relative path for an app-origin path, or `None` when it has a segment that could leave the
/// folder.
fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.contains(['\\', ':']) {
        return None;
    }
    let mut out = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        out.push(segment);
    }
    Some(out)
}

/// Content type from the file extension, the same mapping `embed_dir!` uses.
fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Watches a folder for changes.
#[derive(Debug)]
pub struct Watch {
    root: PathBuf,
    last: u64,
}

impl Watch {
    /// Starts watching `root` from its current state, so the first [`Watch::changed`] call reports
    /// only what changed after this.
    pub fn new(root: PathBuf) -> Self {
        let last = fingerprint(&root);
        Watch { root, last }
    }

    /// Whether a file under the folder was added, removed or changed since the last call.
    pub fn changed(&mut self) -> bool {
        let now = fingerprint(&self.root);
        let changed = now != self.last;
        self.last = now;
        changed
    }
}

/// A hash of the path, size and modification time of every file under `root`, down to
/// [`MAX_DEPTH`] and [`MAX_FILES`]. A file that is added, removed, resized or saved changes it.
pub fn fingerprint(root: &Path) -> u64 {
    let mut files: Vec<(PathBuf, u64, u128)> = Vec::new();
    collect(root, 0, &mut files);
    files.sort();
    let mut hasher = DefaultHasher::new();
    files.hash(&mut hasher);
    hasher.finish()
}

fn collect(dir: &Path, depth: usize, files: &mut Vec<(PathBuf, u64, u128)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if files.len() >= MAX_FILES {
            return;
        }
        // The file type comes from the entry, so a link is not followed.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            collect(&path, depth + 1, files);
        } else if kind.is_file() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            files.push((path, meta.len(), modified));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the temp dir, unique to this test and process.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-dev-reload-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("js")).unwrap();
        fs::write(dir.join("index.html"), b"<p>one</p>").unwrap();
        fs::write(dir.join("js/app.js"), b"1").unwrap();
        dir
    }

    #[test]
    fn the_variable_is_read_only_in_a_debug_build_and_only_for_a_folder() {
        let dir = scratch("env");
        assert_eq!(dev_dir_from(None, true), None);
        assert_eq!(dev_dir_from(Some(dir.clone().into()), false), None);
        assert_eq!(dev_dir_from(Some(dir.join("missing").into()), true), None);
        assert_eq!(
            dev_dir_from(Some(dir.clone().into()), true),
            Some(dir.clone())
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_is_read_from_the_folder_with_its_content_type() {
        let dir = scratch("read");
        let page = disk_asset(&dir, "index.html").expect("index.html");
        assert_eq!(page.content_type, "text/html; charset=utf-8");
        assert_eq!(page.body, b"<p>one</p>");
        let script = disk_asset(&dir, "js/app.js").expect("js/app.js");
        assert_eq!(script.content_type, "text/javascript; charset=utf-8");
        assert!(disk_asset(&dir, "missing.html").is_none());
        assert!(disk_asset(&dir, "js").is_none(), "a folder is not an asset");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_that_could_leave_the_folder_is_refused() {
        let dir = scratch("escape");
        let secret = dir
            .parent()
            .unwrap()
            .join(format!("tpt-dev-reload-secret-{}.txt", std::process::id()));
        fs::write(&secret, b"outside").unwrap();
        let name = secret.file_name().unwrap().to_str().unwrap().to_string();
        for bad in [
            "../".to_string() + &name,
            format!("js/../../{name}"),
            format!("..\\{name}"),
            format!("js\\..\\..\\{name}"),
            secret.to_str().unwrap().to_string(),
            "C:/Windows/win.ini".to_string(),
            "index.html:stream".to_string(),
            "js//app.js".to_string(),
            "./index.html".to_string(),
            String::new(),
        ] {
            assert!(disk_asset(&dir, &bad).is_none(), "served `{bad}`");
        }
        let _ = fs::remove_file(&secret);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_to_any_file_is_seen_once() {
        let dir = scratch("watch");
        let mut watch = Watch::new(dir.clone());
        assert!(!watch.changed(), "nothing changed yet");

        fs::write(dir.join("js/app.js"), b"22").unwrap();
        assert!(watch.changed(), "a resized file is a change");
        assert!(!watch.changed(), "a change is reported once");

        fs::write(dir.join("new.css"), b"body{}").unwrap();
        assert!(watch.changed(), "an added file is a change");

        fs::remove_file(dir.join("new.css")).unwrap();
        assert!(watch.changed(), "a removed file is a change");
        let _ = fs::remove_dir_all(&dir);
    }
}
