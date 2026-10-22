// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Raw printing through the operating system's print spooler (spec §10.2, M7), for a printer
//! that is installed on the system rather than attached as a byte stream.
//!
//! Support: Full on Windows (the winspool RAW datatype, no filter). macOS and Linux are Partial:
//! they go through CUPS `lp` with `-o raw`, which needs CUPS installed and has not been run on
//! those systems from this workspace. Other platforms return [`DeviceError::NotSupported`].

use crate::transport::DeviceError;

/// The name of an installed printer. Validated when it is made, so it is safe to pass to the
/// spooler: printable ASCII, no leading dash, at most 128 characters.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrinterName(String);

impl PrinterName {
    /// Support: Full.
    pub fn new(name: &str) -> Result<Self, DeviceError> {
        let ok = !name.is_empty()
            && name.len() <= 128
            && name == name.trim()
            && !name.starts_with('-')
            && name.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')')
            });
        if ok {
            Ok(PrinterName(name.to_string()))
        } else {
            Err(DeviceError::Denied(format!(
                "`{name}` is not a printer name (1 to 128 letters, digits, spaces and - _ . ( ))"
            )))
        }
    }

    /// Support: Full.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The default printer, if the system has one.
///
/// Support: Partial. Windows: the winspool RAW datatype, tested. macOS and Linux go through CUPS lp
/// with -o raw; not run on those systems from this workspace. Other platforms return NotSupported.
pub fn default_printer() -> Option<PrinterName> {
    let name = platform_default()?;
    PrinterName::new(&name).ok()
}

/// Sends `bytes` to `printer` as one raw job named `job`. The job name is printable ASCII, up
/// to 128 characters.
///
/// Support: Partial. Windows: the winspool RAW datatype, tested. macOS and Linux go through CUPS lp
/// with -o raw; not run on those systems from this workspace. Other platforms return NotSupported.
pub fn send_raw(printer: &PrinterName, job: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    let ok = !job.is_empty()
        && job.len() <= 128
        && job.chars().all(|c| c.is_ascii_graphic() || c == ' ');
    if !ok {
        return Err(DeviceError::Denied(format!(
            "`{job}` is not a job name (1 to 128 printable ASCII characters)"
        )));
    }
    platform_send(printer.as_str(), job, bytes)
}

#[cfg(windows)]
fn platform_default() -> Option<String> {
    crate::platform::win_spool::default_printer()
}

#[cfg(windows)]
fn platform_send(printer: &str, job: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    crate::platform::win_spool::send_raw(printer, job, bytes)
}

#[cfg(unix)]
fn platform_default() -> Option<String> {
    crate::platform::unix_spool::default_printer()
}

#[cfg(unix)]
fn platform_send(printer: &str, job: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    crate::platform::unix_spool::send_raw(printer, job, bytes)
}

#[cfg(not(any(windows, unix)))]
fn platform_default() -> Option<String> {
    None
}

#[cfg(not(any(windows, unix)))]
fn platform_send(_printer: &str, _job: &str, _bytes: &[u8]) -> Result<(), DeviceError> {
    Err(DeviceError::NotSupported(
        "raw printing through the spooler",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printer_names_are_checked_before_they_reach_the_spooler() {
        assert!(PrinterName::new("Receipt Printer (USB)").is_ok());
        assert!(PrinterName::new("EPSON_TM-T20").is_ok());
        let long = "x".repeat(129);
        for bad in [
            "",
            " lead",
            "-o raw",
            "name\nwith newline",
            "a;rm -rf /",
            "../../etc",
            "printer/queue",
            "caf\u{e9}",
            long.as_str(),
        ] {
            assert!(PrinterName::new(bad).is_err(), "accepted `{bad}`");
        }
    }

    #[test]
    fn job_names_are_checked_too() {
        let printer = PrinterName::new("Any").expect("name");
        assert!(matches!(
            send_raw(&printer, "bad\njob", b"x"),
            Err(DeviceError::Denied(_))
        ));
        assert!(matches!(
            send_raw(&printer, "", b"x"),
            Err(DeviceError::Denied(_))
        ));
    }
}
