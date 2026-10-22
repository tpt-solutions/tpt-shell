// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Barcode scanners (spec §10.2, M7). Three ways a scanner reaches the shell, all of them
//! parsed here without touching a device:
//!
//! - keyboard wedge: the scanner types its code into the keyboard stream. [`ScanFramer`]
//!   splits that text into scans on Enter.
//! - HID POS: the scanner sends eight-byte keyboard-style input reports. [`KeyboardReportDecoder`]
//!   turns each report into the characters it types, and the framer splits them.
//! - serial: the scanner sends its code as bytes on a serial port, ended by a carriage return or
//!   line feed. [`read_scans`] reads that through the [`Transport`] trait.
//!
//! Support: Partial. The parsing is portable and tested. HID POS decodes only boot-protocol
//! keyboard reports, so a scanner that needs its own report descriptor is not handled.

use crate::transport::{DeviceError, Transport};

/// Longest scan the framer accepts. A longer run without a terminator is dropped, not split.
///
/// Support: Full.
pub const MAX_SCAN_LEN: usize = 256;

/// Why a piece of scanner input was refused.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanError {
    /// A byte that is not printable ASCII, tab, or a terminator.
    NotText(u8),
    /// More than [`MAX_SCAN_LEN`] bytes arrived with no terminator. The run was dropped.
    TooLong,
    /// A HID report was shorter than eight bytes.
    BadReport,
    /// A HID key code this decoder does not map to a character.
    UnknownKey(u8),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::NotText(b) => write!(f, "scanner sent byte 0x{b:02x}, which is not text"),
            ScanError::TooLong => write!(f, "scan is longer than {MAX_SCAN_LEN} bytes"),
            ScanError::BadReport => write!(f, "HID report is shorter than eight bytes"),
            ScanError::UnknownKey(k) => write!(f, "HID key 0x{k:02x} is not mapped"),
        }
    }
}

impl std::error::Error for ScanError {}

/// Splits a text stream into scans. Carriage return and line feed end a scan; empty scans
/// (such as the line feed after a carriage return) are dropped.
///
/// Support: Full.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanFramer {
    pending: Vec<u8>,
    /// Set after a refused byte or an overlong run: bytes are skipped up to the next terminator.
    dropping: bool,
    /// Set when the run was dropped for length, so the terminator reports it.
    too_long: bool,
}

impl ScanFramer {
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes and returns each completed scan, or the error for a refused run.
    ///
    /// Support: Full.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<String, ScanError>> {
        let mut out = Vec::new();
        for &b in bytes {
            match b {
                b'\r' | b'\n' => {
                    if self.too_long {
                        out.push(Err(ScanError::TooLong));
                    } else if !self.dropping && !self.pending.is_empty() {
                        // Only printable ASCII and tab are ever stored, so this is valid UTF-8.
                        let text = String::from_utf8(std::mem::take(&mut self.pending))
                            .unwrap_or_default();
                        out.push(Ok(text));
                    }
                    self.pending.clear();
                    self.dropping = false;
                    self.too_long = false;
                }
                b'\t' | b' '..=b'~' if !self.dropping => {
                    if self.pending.len() >= MAX_SCAN_LEN {
                        self.pending.clear();
                        self.dropping = true;
                        self.too_long = true;
                    } else {
                        self.pending.push(b);
                    }
                }
                b'\t' | b' '..=b'~' => {}
                other => {
                    // Fail closed: drop this scan and report the byte once.
                    if !self.dropping {
                        out.push(Err(ScanError::NotText(other)));
                    }
                    self.pending.clear();
                    self.dropping = true;
                }
            }
        }
        out
    }
}

/// Reads the input reports of a HID POS scanner. Boot-protocol keyboard layout: byte 0 holds
/// modifiers, byte 1 is reserved, bytes 2 to 7 hold up to six key codes that are down.
///
/// Support: Partial. HID POS decodes only boot-protocol keyboard reports, so a scanner that needs
/// its own report descriptor is not handled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyboardReportDecoder {
    previous: [u8; 6],
}

impl KeyboardReportDecoder {
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the characters typed by a report. A key is typed when it goes down, so a key
    /// that is still held in the next report is not typed again.
    ///
    /// Support: Partial. HID POS decodes only boot-protocol keyboard reports, so a scanner that
    /// needs its own report descriptor is not handled.
    pub fn push_report(&mut self, report: &[u8]) -> Result<String, ScanError> {
        if report.len() < 8 {
            return Err(ScanError::BadReport);
        }
        let shift = report[0] & 0x22 != 0;
        let mut keys = [0u8; 6];
        keys.copy_from_slice(&report[2..8]);
        let mut text = String::new();
        for &key in keys.iter().filter(|k| **k != 0) {
            if self.previous.contains(&key) {
                continue;
            }
            match usage_to_char(key, shift) {
                Some(c) => text.push(c),
                None => {
                    self.previous = keys;
                    return Err(ScanError::UnknownKey(key));
                }
            }
        }
        self.previous = keys;
        Ok(text)
    }
}

/// The character a HID keyboard usage types, with or without shift. Enter is line feed, which the
/// framer treats as the end of a scan.
fn usage_to_char(usage: u8, shift: bool) -> Option<char> {
    let c = match usage {
        0x04..=0x1D => {
            let base = b'a' + (usage - 0x04);
            if shift {
                base.to_ascii_uppercase()
            } else {
                base
            }
        }
        0x1E..=0x27 => {
            const PLAIN: &[u8; 10] = b"1234567890";
            const SHIFTED: &[u8; 10] = b"!@#$%^&*()";
            let i = usize::from(usage - 0x1E);
            if shift {
                SHIFTED[i]
            } else {
                PLAIN[i]
            }
        }
        0x28 => b'\n',
        0x2B => b'\t',
        0x2C => b' ',
        0x2D..=0x38 => {
            // Punctuation keys, in usage order. Each pair is (plain, shifted).
            const PUNCT: [(u8, u8); 12] = [
                (b'-', b'_'),
                (b'=', b'+'),
                (b'[', b'{'),
                (b']', b'}'),
                (b'\\', b'|'),
                (b'#', b'~'),
                (b';', b':'),
                (b'\'', b'"'),
                (b'`', b'~'),
                (b',', b'<'),
                (b'.', b'>'),
                (b'/', b'?'),
            ];
            let (plain, shifted) = PUNCT[usize::from(usage - 0x2D)];
            if shift {
                shifted
            } else {
                plain
            }
        }
        _ => return None,
    };
    Some(char::from(c))
}

/// Reads whatever a serial scanner has sent and returns the scans it completed. Errors from the
/// framer are returned in place, so a bad byte does not hide the scans around it.
///
/// Support: Full.
pub fn read_scans(
    transport: &mut dyn Transport,
    framer: &mut ScanFramer,
) -> Result<Vec<Result<String, ScanError>>, DeviceError> {
    let bytes = transport.read(512)?;
    Ok(framer.push(&bytes))
}

/// Reads HID POS input reports from `transport` and returns the scans they completed.
///
/// Support: Partial. HID POS decodes only boot-protocol keyboard reports, so a scanner that needs
/// its own report descriptor is not handled.
pub fn read_hid_pos_scans(
    transport: &mut dyn Transport,
    decoder: &mut KeyboardReportDecoder,
    framer: &mut ScanFramer,
) -> Result<Vec<Result<String, ScanError>>, DeviceError> {
    let bytes = transport.read(8 * 16)?;
    let mut out = Vec::new();
    for report in bytes.chunks(8) {
        match decoder.push_report(report) {
            Ok(text) => out.extend(framer.push(text.as_bytes())),
            Err(e) => out.push(Err(e)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DeviceId;
    use crate::virtual_devices::VirtualScanner;

    fn scans(results: Vec<Result<String, ScanError>>) -> Vec<String> {
        results.into_iter().filter_map(Result::ok).collect()
    }

    #[test]
    fn carriage_return_and_line_feed_both_end_a_scan() {
        let mut framer = ScanFramer::new();
        let got = framer.push(b"5901234123457\r\n4006381333931\r");
        assert_eq!(scans(got), vec!["5901234123457", "4006381333931"]);
    }

    #[test]
    fn a_scan_split_across_reads_is_joined() {
        let mut framer = ScanFramer::new();
        assert!(framer.push(b"59012").is_empty());
        assert_eq!(scans(framer.push(b"34\r")), vec!["5901234"]);
    }

    #[test]
    fn a_control_byte_is_refused_and_the_run_is_dropped() {
        let mut framer = ScanFramer::new();
        let got = framer.push(b"AB\x01CD\rEF\r");
        assert_eq!(got[0], Err(ScanError::NotText(0x01)));
        assert_eq!(got[1], Ok("EF".to_string()));
        assert_eq!(got.len(), 2, "the dropped run is not returned: {got:?}");
    }

    #[test]
    fn an_overlong_run_is_refused_not_split() {
        let mut framer = ScanFramer::new();
        let long = vec![b'9'; MAX_SCAN_LEN + 10];
        let mut input = long.clone();
        input.push(b'\r');
        input.extend_from_slice(b"OK\r");
        let got = framer.push(&input);
        assert_eq!(got, vec![Err(ScanError::TooLong), Ok("OK".to_string())]);
    }

    #[test]
    fn keyboard_reports_type_their_characters_once() {
        let mut decoder = KeyboardReportDecoder::new();
        // Shift + 'a' (usage 0x04) goes down, then is held.
        let shifted_a = [0x02, 0, 0x04, 0, 0, 0, 0, 0];
        assert_eq!(decoder.push_report(&shifted_a).unwrap(), "A");
        assert_eq!(decoder.push_report(&shifted_a).unwrap(), "");
        // Release, then '1' (0x1E) and '-' (0x2D).
        let release = [0; 8];
        assert_eq!(decoder.push_report(&release).unwrap(), "");
        let digit_and_dash = [0, 0, 0x1E, 0x2D, 0, 0, 0, 0];
        assert_eq!(decoder.push_report(&digit_and_dash).unwrap(), "1-");
    }

    #[test]
    fn a_keyboard_enter_ends_a_scan() {
        let mut decoder = KeyboardReportDecoder::new();
        let mut framer = ScanFramer::new();
        let mut got = Vec::new();
        for report in [
            [0, 0, 0x1E, 0, 0, 0, 0, 0], // 1
            [0; 8],
            [0, 0, 0x1F, 0, 0, 0, 0, 0], // 2
            [0; 8],
            [0, 0, 0x28, 0, 0, 0, 0, 0], // Enter
            [0; 8],
        ] {
            let text = decoder.push_report(&report).expect("report");
            got.extend(framer.push(text.as_bytes()));
        }
        assert_eq!(scans(got), vec!["12"]);
    }

    #[test]
    fn short_reports_and_unmapped_keys_are_refused() {
        let mut decoder = KeyboardReportDecoder::new();
        assert_eq!(
            decoder.push_report(&[0, 0, 0x04]),
            Err(ScanError::BadReport)
        );
        // F1 is usage 0x3A, which is not mapped.
        assert_eq!(
            decoder.push_report(&[0, 0, 0x3A, 0, 0, 0, 0, 0]),
            Err(ScanError::UnknownKey(0x3A))
        );
    }

    #[test]
    fn a_serial_scanner_is_read_through_the_transport() {
        let id: DeviceId = "hid:0c2e:0b61".parse().expect("id");
        let (mut scanner, input) = VirtualScanner::new(id);
        input.scan("5901234123457");
        input.scan("4006381333931");
        let mut framer = ScanFramer::new();
        let mut got = Vec::new();
        for _ in 0..8 {
            got.extend(read_scans(&mut scanner, &mut framer).expect("read"));
        }
        assert_eq!(scans(got), vec!["5901234123457", "4006381333931"]);
    }
}
