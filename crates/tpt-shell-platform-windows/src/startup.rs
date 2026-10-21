// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Starting an app when the user signs in (spec M6, "install-service"). The entry is a value in
//! the user's `Run` key, so it needs no administrator rights and runs only for that user.

use crate::registry;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Adds a sign-in entry called `name` that runs `command`. Quote paths that contain spaces.
pub fn enable(name: &str, command: &str) -> Result<(), String> {
    check_name(name)?;
    registry::set_string(RUN_KEY, Some(name), command)
}

/// Removes the sign-in entry `name`. Removing one that is not there is not an error.
pub fn disable(name: &str) -> Result<(), String> {
    check_name(name)?;
    registry::delete_value(RUN_KEY, name)
}

/// The command of the sign-in entry `name`, or `None` when there is none.
pub fn command(name: &str) -> Option<String> {
    registry::get_string(RUN_KEY, Some(name))
}

/// Entry names are letters, digits, spaces, `-`, `.` and `_`, and are not empty.
fn check_name(name: &str) -> Result<(), String> {
    let ok = !name.trim().is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" -._".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("{name:?} is not a valid startup entry name"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_entry_is_added_read_and_removed() {
        let name = format!("tpt-startup-test-{}", std::process::id());
        let command = r#""C:\Program Files\tpt-app\app.exe" --background"#;
        enable(&name, command).expect("enable");
        assert_eq!(self::command(&name).as_deref(), Some(command));
        disable(&name).expect("disable");
        assert_eq!(self::command(&name), None);
        disable(&name).expect("disabling twice is fine");
    }

    #[test]
    fn a_bad_entry_name_is_refused() {
        for bad in ["", "   ", "a\\b", "x=y"] {
            assert!(enable(bad, "x.exe").is_err(), "{bad:?} was accepted");
        }
    }
}
