// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Raw printing on macOS and Linux through the CUPS `lp` command (spec §10.2, M7). The bytes go
//! to `lp` on standard input with `-o raw`, so no filter touches them.
//!
//! No shell runs, and the printer and job names are passed as separate arguments. The names are
//! validated by the caller first. Support is Partial: this needs CUPS (`lp` on the path), and
//! it has been compile-checked for macOS and Linux but not run on either here.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::transport::DeviceError;

/// Sends `bytes` to `printer` as one raw job named `job`.
pub fn send_raw(printer: &str, job: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    let mut child = Command::new("lp")
        .args(["-d", printer, "-o", "raw", "-t", job, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            DeviceError::Io(format!(
                "cannot run lp ({e}); install CUPS to print on this system"
            ))
        })?;
    let write = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(bytes),
        None => Ok(()),
    };
    let output = child
        .wait_with_output()
        .map_err(|e| DeviceError::Io(format!("lp did not finish: {e}")))?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(DeviceError::Io(format!(
            "lp failed for printer `{printer}`: {}",
            why.trim()
        )));
    }
    write.map_err(|e| DeviceError::Io(format!("sending to lp failed: {e}")))
}

/// The default printer, as `lpstat -d` names it, if CUPS has one.
pub fn default_printer() -> Option<String> {
    let output = Command::new("lpstat").arg("-d").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let name = text.split_once(": ")?.1.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_refuses_a_printer_that_does_not_exist() {
        // Runs on CI runners: with CUPS absent the spawn fails, and with CUPS present lp
        // rejects the unknown destination. Either way nothing is printed.
        let result = send_raw("tpt-no-such-printer-7f3a91", "tpt-test", b"x");
        assert!(result.is_err());
    }
}
