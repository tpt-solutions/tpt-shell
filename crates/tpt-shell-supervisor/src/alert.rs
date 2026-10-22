// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Where the supervisor raises an alert when it gives up (spec M6): a command run with the
//! message as its argument, or a file that gets the message as a new line.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    /// Run this program with the message as its only argument.
    Command(PathBuf),
    /// Append the message as a line to this file.
    File(PathBuf),
}

impl Alert {
    /// Raises the alert. Failures are returned, so the caller can log them.
    ///
    /// Support: Full.
    pub fn raise(&self, message: &str) -> io::Result<()> {
        match self {
            Alert::Command(program) => {
                let status = Command::new(program).arg(message).status()?;
                if status.success() {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "alert command exited with {status}"
                    )))
                }
            }
            Alert::File(path) => {
                let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                writeln!(file, "{message}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_alert_gets_the_message_as_a_line() {
        let path = std::env::temp_dir().join(format!("tpt-alert-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Alert::File(path.clone())
            .raise("crash loop")
            .expect("raise");
        Alert::File(path.clone()).raise("again").expect("raise");
        let text = std::fs::read_to_string(&path).expect("read");
        assert_eq!(text, "crash loop\nagain\n");
        let _ = std::fs::remove_file(&path);
    }
}
