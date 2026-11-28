// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Deep link registration through a desktop entry (todo M5, decision 4).
//!
//! Writes `<data-home>/applications/<app-id>.desktop` with a `MimeType=x-scheme-handler/...`
//! line for each scheme, so the desktop routes `<scheme>://` links to the app's executable.
//! `update-desktop-database` refreshes the cache when it is installed; without it the entry
//! still works after the next login, so its absence is not an error.
//!
//! Nothing is written outside `<data-home>`. Tests pass a temporary directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What [`register`] did.
///
/// Support: Partial. Writes an XDG desktop entry under the data home. Not run on macOS/Linux here;
/// compile-checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The desktop entry that was written.
    pub entry: PathBuf,
    /// Whether `update-desktop-database` ran successfully.
    pub database_updated: bool,
}

/// Writes the desktop entry for `app_id` and refreshes the database if the tool exists.
///
/// `schemes` are URI schemes such as `myapp`. Each must match the RFC 3986 scheme grammar
/// (letter, then letters, digits, `+`, `-` or `.`). `exec` must be an absolute path with no
/// control characters. Anything else is refused before a file is written.
///
/// Support: Partial. Writes an XDG desktop entry under the data home. Not run on macOS/Linux here;
/// compile-checked.
pub fn register(
    data_home: &Path,
    app_id: &str,
    app_name: &str,
    exec: &Path,
    schemes: &[&str],
) -> io::Result<Registration> {
    if schemes.is_empty() {
        return Err(invalid("at least one scheme is needed"));
    }
    for scheme in schemes {
        if !valid_scheme(scheme) {
            return Err(invalid(
                "a scheme must start with a letter and use only letters, digits, '+', '-' or '.'",
            ));
        }
    }
    if app_id.is_empty()
        || !app_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            "the app id must use only letters, digits, '-', '_' or '.'",
        ));
    }
    if app_name.is_empty() || app_name.chars().any(char::is_control) {
        return Err(invalid(
            "the app name must be non-empty and hold no control characters",
        ));
    }
    let exec_text = exec.to_string_lossy();
    if !exec.is_absolute() || exec_text.chars().any(char::is_control) {
        return Err(invalid(
            "the executable must be an absolute path with no control characters",
        ));
    }

    let applications = data_home.join("applications");
    fs::create_dir_all(&applications)?;
    let entry = applications.join(format!("{app_id}.desktop"));
    let text = desktop_entry(app_name, exec, schemes);
    // Write to a temporary name and rename, so a half-written entry is never visible.
    let tmp = applications.join(format!("{app_id}.desktop.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &entry)?;

    let database_updated = refresh_database(&applications);
    Ok(Registration {
        entry,
        database_updated,
    })
}

/// The data home the entries go under: `$XDG_DATA_HOME` when it is an absolute path, otherwise
/// `$HOME/.local/share`. `None` when neither is an absolute path.
///
/// Support: Full.
pub fn data_home_from(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let absolute = |text: &str| Path::new(text).is_absolute();
    match xdg.filter(|p| absolute(p)) {
        Some(path) => Some(PathBuf::from(path)),
        None => home
            .filter(|p| absolute(p))
            .map(|h| Path::new(h).join(".local").join("share")),
    }
}

/// The data home for this user, read from the environment (see [`data_home_from`]).
///
/// Support: Full.
pub fn data_home() -> Option<PathBuf> {
    data_home_from(
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// The app id an entry is named by: the program's file name, with characters a desktop entry does
/// not accept replaced by `-`.
///
/// Support: Full.
pub fn app_id_for(exe: &Path) -> String {
    exe.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "tpt-shell".into())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Where the entry for `app_id` lives under `data_home`.
///
/// Support: Full.
pub fn entry_path(data_home: &Path, app_id: &str) -> PathBuf {
    data_home
        .join("applications")
        .join(format!("{app_id}.desktop"))
}

/// Writes the entry for `schemes`, or removes it when there are none. Returns whether an entry
/// was written. A missing entry is not an error when it is being removed.
///
/// Support: Partial. Writes or removes one file under `data_home`. Not run on macOS/Linux here;
/// compile-checked.
pub fn write_or_remove(
    data_home: &Path,
    app_id: &str,
    exec: &Path,
    schemes: &[String],
) -> io::Result<bool> {
    if schemes.is_empty() {
        return match fs::remove_file(entry_path(data_home, app_id)) {
            Ok(()) => Ok(false),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        };
    }
    let refs: Vec<&str> = schemes.iter().map(String::as_str).collect();
    register(data_home, app_id, app_id, exec, &refs)?;
    Ok(true)
}

/// Builds the text of a desktop entry. Exposed so the format can be tested directly.
///
/// Support: Full.
pub fn desktop_entry(app_name: &str, exec: &Path, schemes: &[&str]) -> String {
    let mime: String = schemes
        .iter()
        .map(|s| format!("x-scheme-handler/{s};"))
        .collect();
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nTerminal=false\nNoDisplay=true\nMimeType={mime}\n",
        escape_string(app_name),
        exec_value(exec),
    )
}

/// The `Exec` value: the path in double quotes, with the characters the Exec grammar reserves
/// escaped. The desktop-entry string escaping (backslash doubling) is applied on top.
fn exec_value(exec: &Path) -> String {
    let mut quoted = String::from("\"");
    for ch in exec.to_string_lossy().chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted.push('"');
    format!("{} %u", escape_string(&quoted))
}

/// Desktop-entry string escaping: a backslash is written as two.
fn escape_string(value: &str) -> String {
    value.replace('\\', "\\\\")
}

/// RFC 3986 scheme grammar, lowercase only.
fn valid_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    scheme.len() <= 64
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Runs `update-desktop-database` on `dir` if it is on the PATH. Returns whether it succeeded.
fn refresh_database(dir: &Path) -> bool {
    let Some(tool) = find_on_path("update-desktop-database") else {
        return false;
    };
    Command::new(tool)
        .arg(dir)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-shell-desktop-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn entry_declares_one_handler_per_scheme() {
        let text = desktop_entry("Demo", Path::new("/opt/demo/demo"), &["demo", "demo-dev"]);
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("MimeType=x-scheme-handler/demo;x-scheme-handler/demo-dev;\n"));
        assert!(text.contains("Exec=\"/opt/demo/demo\" %u\n"));
        assert!(text.contains("Name=Demo\n"));
    }

    #[test]
    fn exec_escapes_reserved_characters_and_backslashes() {
        let text = desktop_entry("A", Path::new("/opt/a $b/x\"y`z\\w"), &["a"]);
        // Exec quoting escapes $, ", ` and \; the desktop string escaping then doubles each
        // backslash it finds, so the single escape becomes two.
        assert!(
            text.contains(r#"Exec="/opt/a \\$b/x\\"y\\`z\\\\w" %u"#),
            "{text}"
        );
    }

    #[test]
    fn refused_inputs_write_nothing() {
        let dir = temp_dir("refuse");
        let exec = Path::new("/opt/demo");
        assert!(register(&dir, "demo", "Demo", exec, &[]).is_err());
        assert!(register(&dir, "demo", "Demo", exec, &["Bad Scheme"]).is_err());
        assert!(register(&dir, "demo", "Demo", exec, &["1abc"]).is_err());
        assert!(register(&dir, "../demo", "Demo", exec, &["demo"]).is_err());
        assert!(register(&dir, "demo", "De\nmo", exec, &["demo"]).is_err());
        assert!(register(&dir, "demo", "Demo", Path::new("demo"), &["demo"]).is_err());
        assert!(
            !dir.join("applications").exists(),
            "nothing is written on refusal"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn register_writes_the_entry_under_the_data_home() {
        let dir = temp_dir("write");
        let exec = dir.join("demo-bin");
        let reg = register(&dir, "demo", "Demo", &exec, &["demo"]).unwrap();
        assert_eq!(reg.entry, dir.join("applications").join("demo.desktop"));
        let text = fs::read_to_string(&reg.entry).unwrap();
        assert!(text.contains("MimeType=x-scheme-handler/demo;"));
        assert!(!dir.join("applications").join("demo.desktop.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scheme_grammar_is_enforced() {
        assert!(valid_scheme("myapp"));
        assert!(valid_scheme("my-app.v2+x"));
        assert!(!valid_scheme(""));
        assert!(!valid_scheme("My"));
        assert!(!valid_scheme("2fa"));
        assert!(!valid_scheme("a:b"));
    }

    #[test]
    fn the_data_home_follows_xdg_then_home_and_needs_absolute_paths() {
        let abs = std::env::temp_dir();
        let abs_text = abs.to_string_lossy().into_owned();
        assert_eq!(
            data_home_from(Some(abs_text.as_str()), Some(abs_text.as_str())),
            Some(abs.clone())
        );
        assert_eq!(
            data_home_from(Some("relative/dir"), Some(abs_text.as_str())),
            Some(abs.join(".local").join("share"))
        );
        assert_eq!(data_home_from(Some("relative"), None), None);
        assert_eq!(data_home_from(None, None), None);
    }

    #[test]
    fn write_or_remove_writes_the_list_and_removes_the_entry_when_empty() {
        let home = temp_dir("write-remove");
        let exec = std::env::temp_dir().join("demo");
        let one = vec!["demo".to_string()];
        assert!(write_or_remove(&home, "demo", &exec, &one).expect("write"));
        let entry = entry_path(&home, "demo");
        let text = fs::read_to_string(&entry).expect("entry");
        assert!(text.contains("x-scheme-handler/demo"), "{text}");

        assert!(!write_or_remove(&home, "demo", &exec, &[]).expect("remove"));
        assert!(!entry.exists());
        // Removing an entry that is already gone is not an error.
        assert!(!write_or_remove(&home, "demo", &exec, &[]).expect("remove again"));
    }

    #[test]
    fn app_ids_replace_characters_a_desktop_entry_refuses() {
        assert_eq!(app_id_for(Path::new("/x/My App.bin")), "My-App");
        assert_eq!(app_id_for(Path::new("/x/ok-name")), "ok-name");
    }
}
