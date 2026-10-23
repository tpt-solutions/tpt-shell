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
    /// A keyboard-wedge scan was still open when the next key came too late. The partial scan
    /// was dropped.
    Timeout,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::NotText(b) => write!(f, "scanner sent byte 0x{b:02x}, which is not text"),
            ScanError::TooLong => write!(f, "scan is longer than {MAX_SCAN_LEN} bytes"),
            ScanError::BadReport => write!(f, "HID report is shorter than eight bytes"),
            ScanError::UnknownKey(k) => write!(f, "HID key 0x{k:02x} is not mapped"),
            ScanError::Timeout => write!(f, "keyboard scan timed out and was dropped"),
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
    /// Text a scanner adds in front of every code (such as a symbology prefix). Removed when present.
    prefix: String,
    /// Text a scanner adds after every code (such as a checksum tag). Removed when present.
    suffix: String,
}

impl ScanFramer {
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// A framer that removes `prefix` from the start and `suffix` from the end of every scan. An
    /// affix that is not there is left alone, and an empty string removes nothing.
    ///
    /// Support: Full. Tested with and without each affix, and with a scan that is only an affix.
    pub fn with_affixes(prefix: &str, suffix: &str) -> Self {
        ScanFramer {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            ..Self::default()
        }
    }

    /// Returns the pending run as a scan, with the affixes removed, and clears it.
    fn take_scan(&mut self) -> String {
        // Only printable ASCII and tab are ever stored, so this is valid UTF-8.
        let raw = String::from_utf8(std::mem::take(&mut self.pending)).unwrap_or_default();
        let body = raw.strip_prefix(self.prefix.as_str()).unwrap_or(&raw);
        let body = body.strip_suffix(self.suffix.as_str()).unwrap_or(body);
        body.to_string()
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
                        let text = self.take_scan();
                        if !text.is_empty() {
                            out.push(Ok(text));
                        }
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

/// One key press from a keyboard-wedge scanner: a character it types, or Enter.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WedgeKey {
    /// A character key. Printable ASCII and tab are text; anything else refuses the scan.
    Char(char),
    /// The Enter key, which ends a scan.
    Enter,
}

/// Turns the key presses of a keyboard-wedge scanner into scans. Every key carries the time it was
/// pressed, in milliseconds on a clock the caller chooses, so the timeout can be tested without a
/// real clock.
///
/// When a key arrives more than the timeout after the previous one while a scan is open, the open
/// scan is dropped and [`ScanError::Timeout`] is returned. A half-typed code is therefore never
/// joined to the code that follows it.
///
/// Support: Partial. Scan text is not affix-stripped here; use [`ScanFramer::with_affixes`] on the
/// text if a scanner adds prefixes. The timeout must be longer than the scanner's delay between keys,
/// which the caller sets; a person typing by hand can also hit it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardWedge {
    timeout_ms: u64,
    last_key_ms: Option<u64>,
    pending: String,
    /// Set after a refused character or an overlong scan: keys are skipped up to Enter.
    dropping: bool,
}

impl KeyboardWedge {
    /// A decoder that drops an open scan after `timeout_ms` without a key.
    ///
    /// Support: Full.
    pub fn new(timeout_ms: u64) -> Self {
        KeyboardWedge {
            timeout_ms,
            last_key_ms: None,
            pending: String::new(),
            dropping: false,
        }
    }

    /// Feeds one key pressed at `at_ms`. Returns a scan when Enter completes one. Returns an error
    /// when a scan was dropped: after a timeout, for a character that is not text, or for a run
    /// longer than [`MAX_SCAN_LEN`].
    ///
    /// Support: Partial. Scan text is not affix-stripped here; use [`ScanFramer::with_affixes`] on
    /// the text if a scanner adds prefixes.
    pub fn push(&mut self, key: WedgeKey, at_ms: u64) -> Option<Result<String, ScanError>> {
        let mut timed_out = None;
        if let Some(last) = self.last_key_ms {
            if !self.pending.is_empty() && at_ms.saturating_sub(last) > self.timeout_ms {
                self.pending.clear();
                self.dropping = false;
                timed_out = Some(Err(ScanError::Timeout));
            }
        }
        self.last_key_ms = Some(at_ms);
        let completed = match key {
            WedgeKey::Char(c) => self.character(c),
            WedgeKey::Enter => self.enter(),
        };
        timed_out.or(completed)
    }

    fn character(&mut self, c: char) -> Option<Result<String, ScanError>> {
        if self.dropping {
            return None;
        }
        if !(c.is_ascii_graphic() || c == ' ' || c == '\t') {
            self.pending.clear();
            self.dropping = true;
            let mut buf = [0u8; 4];
            return Some(Err(ScanError::NotText(
                c.encode_utf8(&mut buf).as_bytes()[0],
            )));
        }
        if self.pending.len() >= MAX_SCAN_LEN {
            self.pending.clear();
            self.dropping = true;
            return Some(Err(ScanError::TooLong));
        }
        self.pending.push(c);
        None
    }

    fn enter(&mut self) -> Option<Result<String, ScanError>> {
        let dropping = std::mem::take(&mut self.dropping);
        let text = std::mem::take(&mut self.pending);
        if dropping || text.is_empty() {
            return None;
        }
        Some(Ok(text))
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

    #[test]
    fn affixes_are_removed_only_when_present() {
        let mut framer = ScanFramer::with_affixes("]C1", "X");
        let got = framer.push(b"]C15901X\r12X\r]C1\r");
        // The first scan loses both affixes, the second has no prefix and keeps its text, and
        // the third is only an affix, so nothing is returned for it.
        assert_eq!(scans(got), vec!["5901", "12"]);
    }

    #[test]
    fn a_plain_framer_keeps_the_whole_scan() {
        let mut framer = ScanFramer::new();
        assert_eq!(scans(framer.push(b"]C15901\r")), vec!["]C15901"]);
    }

    #[test]
    fn a_wedge_scan_completes_on_enter() {
        let mut wedge = KeyboardWedge::new(500);
        assert_eq!(wedge.push(WedgeKey::Char('1'), 0), None);
        assert_eq!(wedge.push(WedgeKey::Char('2'), 30), None);
        assert_eq!(wedge.push(WedgeKey::Enter, 60), Some(Ok("12".to_string())));
        assert_eq!(
            wedge.push(WedgeKey::Enter, 70),
            None,
            "an empty Enter is nothing"
        );
    }

    #[test]
    fn a_wedge_drops_a_partial_scan_after_the_timeout() {
        let mut wedge = KeyboardWedge::new(500);
        assert_eq!(wedge.push(WedgeKey::Char('1'), 0), None);
        // The key at 2000 ms comes too late: the open scan "1" is dropped, and "2" starts afresh.
        assert_eq!(
            wedge.push(WedgeKey::Char('2'), 2000),
            Some(Err(ScanError::Timeout))
        );
        assert_eq!(wedge.push(WedgeKey::Char('3'), 2010), None);
        assert_eq!(
            wedge.push(WedgeKey::Enter, 2020),
            Some(Ok("23".to_string()))
        );
    }

    #[test]
    fn a_wedge_refuses_a_non_text_character_and_skips_to_enter() {
        let mut wedge = KeyboardWedge::new(500);
        assert_eq!(wedge.push(WedgeKey::Char('4'), 0), None);
        assert_eq!(
            wedge.push(WedgeKey::Char('\u{e9}'), 10),
            Some(Err(ScanError::NotText(0xC3)))
        );
        assert_eq!(wedge.push(WedgeKey::Char('5'), 20), None);
        assert_eq!(
            wedge.push(WedgeKey::Enter, 30),
            None,
            "the refused scan is not returned"
        );
        assert_eq!(wedge.push(WedgeKey::Char('6'), 40), None);
        assert_eq!(wedge.push(WedgeKey::Enter, 50), Some(Ok("6".to_string())));
    }

    #[test]
    fn a_wedge_refuses_an_overlong_run_once() {
        let mut wedge = KeyboardWedge::new(10_000);
        let mut results = Vec::new();
        for i in 0..=MAX_SCAN_LEN as u64 {
            results.extend(wedge.push(WedgeKey::Char('9'), i));
        }
        assert_eq!(results, vec![Err(ScanError::TooLong)]);
        assert_eq!(
            wedge.push(WedgeKey::Enter, MAX_SCAN_LEN as u64 + 1),
            None,
            "the overlong scan is not returned"
        );
    }
}
