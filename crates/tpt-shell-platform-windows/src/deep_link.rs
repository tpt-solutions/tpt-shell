// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Deep links on Windows (spec §6.1, the `Platform` trait). A URL scheme such as `myapp://` is
//! registered for the current user under `HKCU\Software\Classes`, and the link starts the app
//! with the URL as its argument. A running app receives it through single instance.

use std::path::Path;

use crate::registry;

/// Schemes a page or another app already relies on. Registering one would take it over.
const RESERVED: &[&str] = &[
    "http",
    "https",
    "file",
    "javascript",
    "data",
    "about",
    "mailto",
    "ms",
];

/// Registers `scheme` for the current user, so that links to it start `exe` with the link as
/// the argument.
pub fn register(scheme: &str, exe: &Path, description: &str) -> Result<(), String> {
    check_scheme(scheme)?;
    let root = format!(r"Software\Classes\{scheme}");
    let command = format!("\"{}\" \"%1\"", exe.display());
    registry::set_string(&root, None, &format!("URL:{description}"))?;
    registry::set_string(&root, Some("URL Protocol"), "")?;
    registry::set_string(&format!(r"{root}\shell\open\command"), None, &command)?;
    Ok(())
}

/// Removes a registration made by [`register`]. Removing one that is not there is not an error.
pub fn unregister(scheme: &str) -> Result<(), String> {
    check_scheme(scheme)?;
    registry::delete_tree(&format!(r"Software\Classes\{scheme}"))
}

/// The command a scheme starts, or `None` when it is not registered.
pub fn registered_command(scheme: &str) -> Option<String> {
    registry::get_string(
        &format!(r"Software\Classes\{scheme}\shell\open\command"),
        None,
    )
}

/// Schemes are a letter followed by letters, digits, `+`, `-` or `.`, and are not reserved.
fn check_scheme(scheme: &str) -> Result<(), String> {
    let mut chars = scheme.chars();
    let starts_well = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !starts_well || !rest_ok {
        return Err(format!("{scheme:?} is not a valid URL scheme"));
    }
    if RESERVED.contains(&scheme.to_ascii_lowercase().as_str()) {
        return Err(format!("{scheme:?} is reserved and cannot be registered"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scheme_is_registered_to_the_executable_and_removed_again() {
        let scheme = format!("tpttest{}", std::process::id());
        let exe = Path::new(r"C:\Program Files\tpt-app\app.exe");
        register(&scheme, exe, "Test app").expect("register");
        assert_eq!(
            registered_command(&scheme).as_deref(),
            Some(r#""C:\Program Files\tpt-app\app.exe" "%1""#)
        );
        unregister(&scheme).expect("unregister");
        assert_eq!(registered_command(&scheme), None);
        unregister(&scheme).expect("unregistering twice is fine");
    }

    #[test]
    fn reserved_and_malformed_schemes_are_refused() {
        let exe = Path::new(r"C:\app.exe");
        for bad in ["https", "HTTP", "file", "1app", "my app", "a/b", ""] {
            assert!(register(bad, exe, "x").is_err(), "{bad:?} was accepted");
        }
    }
}
