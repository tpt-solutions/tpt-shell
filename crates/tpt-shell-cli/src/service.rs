// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Sign-in entries for macOS (a LaunchAgent) and Linux (a systemd user unit). The file layout
//! comes from `tpt_shell_supervisor::install`. Every function takes the home folder, so tests
//! write into a temporary folder and never the real one. Windows keeps its own entry in
//! `main.rs`, through the platform crate.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tpt_shell_supervisor::install::{self, ServiceSpec};

/// The sign-in mechanism of one operating system.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    LaunchAgent,
    SystemdUser,
}

/// The sign-in mechanism of this operating system, or `None` where the CLI has none.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn platform() -> Option<Platform> {
    if cfg!(target_os = "macos") {
        Some(Platform::LaunchAgent)
    } else if cfg!(target_os = "linux") {
        Some(Platform::SystemdUser)
    } else {
        None
    }
}

/// The user's home folder, from `HOME`.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn home_dir() -> Result<PathBuf, String> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home)),
        _ => Err("HOME is not set; cannot find the user's folder".into()),
    }
}

/// Writes the sign-in file for `spec` under `home`. Returns the path written.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn install(platform: Platform, home: &Path, spec: &ServiceSpec) -> Result<PathBuf, String> {
    let written = match platform {
        Platform::LaunchAgent => {
            install::write_launch_agent(&install::launch_agents_dir(home), spec)
        }
        Platform::SystemdUser => {
            install::write_systemd_unit(&install::systemd_user_dir(home), spec)
        }
    };
    written.map_err(|e| format!("cannot write the sign-in entry: {e}"))
}

/// The sign-in file for the entry called `name`, under `home`. The name is checked the same way
/// as at install time.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn entry_path(platform: Platform, home: &Path, name: &str) -> Result<PathBuf, String> {
    let spec = ServiceSpec::new(name, "", Vec::new()).map_err(|e| e.to_string())?;
    Ok(match platform {
        Platform::LaunchAgent => {
            install::launch_agents_dir(home).join(format!("{}.plist", spec.label()))
        }
        Platform::SystemdUser => install::systemd_user_dir(home).join(spec.unit_name()),
    })
}

/// Removes the sign-in file for `name`. Returns the path removed, or `None` when there was
/// nothing to remove.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn uninstall(platform: Platform, home: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let path = entry_path(platform, home, name)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(Some(path)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

/// The command that starts (or stops) the entry now, for the user to run. The CLI does not run
/// it, because starting a service is a step the user should take.
///
/// Support: Partial. Writes and removes the sign-in file under a given home folder, tested on
/// Windows with temporary folders. The launchd and systemd entries were not run on macOS or Linux
/// here; compile-checked only.
pub fn start_hint(platform: Platform, path: &Path) -> String {
    match platform {
        Platform::LaunchAgent => format!("start it now with: launchctl load -w {}", path.display()),
        Platform::SystemdUser => {
            let unit = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("start it now with: systemctl --user enable --now {unit}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch_home(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("tpt-cli-svc-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spec(name: &str) -> ServiceSpec {
        ServiceSpec::new(name, "/opt/app/app", vec!["--quiet".into()]).unwrap()
    }

    #[test]
    fn a_launch_agent_is_written_under_the_home_folder() {
        let home = scratch_home("plist");
        let path = install(Platform::LaunchAgent, &home, &spec("demo")).unwrap();
        assert_eq!(
            path,
            home.join("Library")
                .join("LaunchAgents")
                .join("com.tpt-shell.demo.plist")
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("<string>/opt/app/app</string>"));
        assert!(text.contains("<string>--quiet</string>"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_systemd_unit_is_written_under_the_home_folder() {
        let home = scratch_home("unit");
        let path = install(Platform::SystemdUser, &home, &spec("demo")).unwrap();
        assert_eq!(
            path,
            home.join(".config")
                .join("systemd")
                .join("user")
                .join("tpt-shell-demo.service")
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("ExecStart=/opt/app/app --quiet"));
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn uninstall_removes_the_entry_and_tolerates_none() {
        let home = scratch_home("remove");
        install(Platform::SystemdUser, &home, &spec("demo")).unwrap();
        let removed = uninstall(Platform::SystemdUser, &home, "demo").unwrap();
        assert!(removed.is_some());
        assert_eq!(
            uninstall(Platform::SystemdUser, &home, "demo").unwrap(),
            None
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_bad_name_is_refused_before_any_path_is_built() {
        let home = scratch_home("badname");
        assert!(entry_path(Platform::LaunchAgent, &home, "../escape").is_err());
        assert!(uninstall(Platform::SystemdUser, &home, "a/b").is_err());
        assert!(ServiceSpec::new("a b", "/x", Vec::new()).is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn the_start_hint_names_the_mechanism() {
        let agent = Path::new("/home/u/Library/LaunchAgents/com.tpt-shell.demo.plist");
        assert!(
            start_hint(Platform::LaunchAgent, agent).starts_with("start it now with: launchctl")
        );
        let unit = Path::new("/home/u/.config/systemd/user/tpt-shell-demo.service");
        assert!(start_hint(Platform::SystemdUser, unit).ends_with("tpt-shell-demo.service"));
    }
}
