// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Virtual devices (spec §10.2, M7): stand-ins that behave like the real thing, in memory, so
//! code that uses a device can be tested on any machine. Each kind has a handle that the test or
//! the demo uses to play the other side (send a scan, read what was printed, and so on).
//!
//! The virtual printer writes ESC/POS bytes, and [`PrinterOutput::preview`] turns them into text
//! for a receipt preview. It understands the commands the [`crate::escpos`] builder emits.

use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::id::DeviceId;
use crate::transport::{DeviceError, Transport};

/// Most bytes a serial loopback holds before it refuses more writes.
const SERIAL_CAPACITY: usize = 64 * 1024;

type Bytes = Arc<Mutex<VecDeque<u8>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A serial port that sends back what it is given. Writes go to its own read queue.
pub struct VirtualSerial {
    id: DeviceId,
    queue: Bytes,
}

impl VirtualSerial {
    pub fn loopback(id: DeviceId) -> Self {
        VirtualSerial {
            id,
            queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

impl Transport for VirtualSerial {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let mut queue = lock(&self.queue);
        if queue.len() + bytes.len() > SERIAL_CAPACITY {
            return Err(DeviceError::Busy);
        }
        queue.extend(bytes.iter().copied());
        Ok(())
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut queue = lock(&self.queue);
        let n = max.min(queue.len());
        Ok(queue.drain(..n).collect())
    }
}

/// A printer. Everything written to it is kept for the preview.
pub struct VirtualPrinter {
    id: DeviceId,
    output: Arc<Mutex<Vec<u8>>>,
}

/// The printer's paper, seen from outside the device.
#[derive(Clone)]
pub struct PrinterOutput(Arc<Mutex<Vec<u8>>>);

impl VirtualPrinter {
    /// The printer and the handle that reads its output.
    pub fn new(id: DeviceId) -> (Self, PrinterOutput) {
        let output = Arc::new(Mutex::new(Vec::new()));
        (
            VirtualPrinter {
                id,
                output: output.clone(),
            },
            PrinterOutput(output),
        )
    }
}

impl Transport for VirtualPrinter {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        lock(&self.output).extend_from_slice(bytes);
        Ok(())
    }

    fn read(&mut self, _max: usize) -> Result<Vec<u8>, DeviceError> {
        Err(DeviceError::NotSupported("reading from a printer"))
    }
}

/// Columns on the preview, the width of a common 80 mm roll in a standard font.
const COLUMNS: usize = 42;

impl PrinterOutput {
    /// The bytes printed so far.
    pub fn bytes(&self) -> Vec<u8> {
        lock(&self.0).clone()
    }

    /// The receipt as text, one line per printed line. A line printed in bold is wrapped in `*`,
    /// centred and right-aligned lines are padded to the preview width, and each cut shows as a
    /// rule. Bytes this interpreter does not know are skipped.
    pub fn preview(&self) -> String {
        let bytes = self.bytes();
        let mut out = String::new();
        let mut line = String::new();
        let mut line_bold = false;
        let mut bold = false;
        let mut align = 0u8;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\n' => {
                    flush_line(&mut out, &mut line, line_bold, align);
                    line_bold = false;
                    i += 1;
                }
                0x1B if i + 1 < bytes.len() && bytes[i + 1] == b'@' => {
                    // ESC @ resets the printer; it has no parameter.
                    bold = false;
                    align = 0;
                    i += 2;
                }
                0x1B if i + 2 < bytes.len() => {
                    match bytes[i + 1] {
                        b'E' => bold = bytes[i + 2] != 0,
                        b'a' => align = bytes[i + 2],
                        b'd' => {
                            for _ in 0..bytes[i + 2] {
                                flush_line(&mut out, &mut line, line_bold, align);
                                line_bold = false;
                            }
                        }
                        _ => {}
                    }
                    i += 3;
                }
                0x1D if i + 2 < bytes.len() && bytes[i + 1] == b'V' => {
                    out.push_str(&format!("{:-^width$}\n", " cut ", width = COLUMNS));
                    i += 3;
                }
                b => {
                    if bold {
                        line_bold = true;
                    }
                    line.push(b as char);
                    i += 1;
                }
            }
        }
        if !line.is_empty() {
            flush_line(&mut out, &mut line, line_bold, align);
        }
        out
    }
}

/// Writes one preview line: trimmed, wrapped in `*` when bold, and padded for its alignment.
fn flush_line(out: &mut String, line: &mut String, bold: bool, align: u8) {
    let text = line.trim_end();
    let text = if bold {
        format!("*{text}*")
    } else {
        text.to_string()
    };
    let padded = match align {
        1 => format!("{:^width$}", text, width = COLUMNS),
        2 => format!("{:>width$}", text, width = COLUMNS),
        _ => text,
    };
    out.push_str(padded.trim_end());
    out.push('\n');
    line.clear();
}

/// A scanner that sends each scan as its text followed by a carriage return, the way a
/// keyboard-wedge scanner types a barcode.
pub struct VirtualScanner {
    id: DeviceId,
    queue: Bytes,
}

/// The scanner's trigger, seen from outside the device.
#[derive(Clone)]
pub struct ScannerInput(Bytes);

impl VirtualScanner {
    pub fn new(id: DeviceId) -> (Self, ScannerInput) {
        let queue: Bytes = Arc::new(Mutex::new(VecDeque::new()));
        (
            VirtualScanner {
                id,
                queue: queue.clone(),
            },
            ScannerInput(queue),
        )
    }
}

impl ScannerInput {
    /// Scans `code`. It is read from the device as `code` followed by a carriage return.
    pub fn scan(&self, code: &str) {
        let mut queue = lock(&self.0);
        queue.extend(code.bytes());
        queue.push_back(b'\r');
    }
}

impl Transport for VirtualScanner {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, _bytes: &[u8]) -> Result<(), DeviceError> {
        Err(DeviceError::NotSupported("writing to a scanner"))
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut queue = lock(&self.queue);
        let n = max.min(queue.len());
        Ok(queue.drain(..n).collect())
    }
}

/// A HID device: input reports go in one direction and output reports in the other.
pub struct VirtualHid {
    id: DeviceId,
    inbox: Arc<Mutex<VecDeque<Vec<u8>>>>,
    outbox: Arc<Mutex<Vec<Vec<u8>>>>,
}

/// The HID device's other side: send input reports, and read the output reports it received.
#[derive(Clone)]
pub struct HidHandle {
    inbox: Arc<Mutex<VecDeque<Vec<u8>>>>,
    outbox: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl VirtualHid {
    pub fn new(id: DeviceId) -> (Self, HidHandle) {
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let outbox = Arc::new(Mutex::new(Vec::new()));
        (
            VirtualHid {
                id,
                inbox: inbox.clone(),
                outbox: outbox.clone(),
            },
            HidHandle { inbox, outbox },
        )
    }
}

impl HidHandle {
    /// Queues one input report for the device to read.
    pub fn send_report(&self, report: &[u8]) {
        lock(&self.inbox).push_back(report.to_vec());
    }

    /// The output reports the device has written, in order.
    pub fn output_reports(&self) -> Vec<Vec<u8>> {
        lock(&self.outbox).clone()
    }
}

impl Transport for VirtualHid {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        lock(&self.outbox).push(bytes.to_vec());
        Ok(())
    }

    /// Returns the next input report, cut to `max` bytes. The rest of a long report is lost, as
    /// it is with a HID read buffer that is too small.
    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut inbox = lock(&self.inbox);
        Ok(match inbox.pop_front() {
            Some(mut report) => {
                report.truncate(max);
                report
            }
            None => Vec::new(),
        })
    }
}

/// The virtual devices a spec names. Each entry is `kind@id`, separated by `;`:
///
/// - `serial@serial:COM7`: a loopback serial port
/// - `printer@usb:0416:5011`: a receipt printer
/// - `scanner@hid:0c2e:0b61`: a barcode scanner
/// - `hid@hid:1234:5678`: a generic HID device
///
/// An empty spec gives no devices. Used with the `TPT_VIRTUAL_DEVICES` environment variable.
pub fn from_spec(spec: &str) -> Result<Vec<Box<dyn Transport>>, String> {
    let mut devices: Vec<Box<dyn Transport>> = Vec::new();
    for entry in spec.split(';').map(str::trim).filter(|e| !e.is_empty()) {
        let (kind, id) = entry
            .split_once('@')
            .ok_or_else(|| format!("{entry:?} needs the form kind@id"))?;
        let id = DeviceId::from_str(id.trim()).map_err(|e| e.to_string())?;
        let device: Box<dyn Transport> = match kind.trim() {
            "serial" => Box::new(VirtualSerial::loopback(id)),
            "printer" => Box::new(VirtualPrinter::new(id).0),
            "scanner" => Box::new(VirtualScanner::new(id).0),
            "hid" => Box::new(VirtualHid::new(id).0),
            other => return Err(format!("{other:?} is not a virtual device kind")),
        };
        devices.push(device);
    }
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escpos::{Align, Receipt};

    fn id(text: &str) -> DeviceId {
        text.parse().expect("id")
    }

    #[test]
    fn serial_loopback_returns_what_was_written_and_refuses_when_full() {
        let mut port = VirtualSerial::loopback(id("serial:COM7"));
        port.write(b"hello").expect("write");
        assert_eq!(port.read(3).expect("read"), b"hel");
        assert_eq!(port.read(10).expect("read rest"), b"lo");
        assert_eq!(port.read(10).expect("nothing"), Vec::<u8>::new());

        let big = vec![0u8; SERIAL_CAPACITY];
        port.write(&big).expect("fills the buffer");
        assert_eq!(port.write(b"x"), Err(DeviceError::Busy));
    }

    #[test]
    fn a_printed_receipt_previews_as_it_would_look() {
        let (mut printer, output) = VirtualPrinter::new(id("usb:0416:5011"));
        let mut r = Receipt::new();
        r.align(Align::Center)
            .bold(true)
            .line("SHOP")
            .unwrap()
            .bold(false)
            .align(Align::Left)
            .line("Tea 2.50")
            .unwrap()
            .cut(false);
        printer.write(&r.into_bytes()).expect("print");
        let preview = output.preview();
        let lines: Vec<&str> = preview.lines().collect();
        assert_eq!(lines[0].trim(), "*SHOP*");
        assert!(
            lines[0].starts_with(' '),
            "centred line is padded: {lines:?}"
        );
        assert_eq!(lines[1], "Tea 2.50");
        assert!(lines[2].contains("cut"));
    }

    #[test]
    fn a_printer_cannot_be_read() {
        let (mut printer, _) = VirtualPrinter::new(id("usb:0416:5011"));
        assert!(matches!(printer.read(1), Err(DeviceError::NotSupported(_))));
    }

    #[test]
    fn a_scan_is_read_as_its_text_and_a_carriage_return() {
        let (mut scanner, input) = VirtualScanner::new(id("hid:0c2e:0b61"));
        input.scan("5901234123457");
        let mut got = Vec::new();
        loop {
            let chunk = scanner.read(4).expect("read");
            if chunk.is_empty() {
                break;
            }
            got.extend(chunk);
        }
        assert_eq!(got, b"5901234123457\r");
    }

    #[test]
    fn hid_reports_go_both_ways() {
        let (mut hid, handle) = VirtualHid::new(id("hid:1234:5678"));
        handle.send_report(&[1, 2, 3]);
        assert_eq!(hid.read(64).expect("read"), vec![1, 2, 3]);
        assert!(hid.read(64).expect("empty").is_empty());
        hid.write(&[9, 9]).expect("write");
        assert_eq!(handle.output_reports(), vec![vec![9, 9]]);
    }

    #[test]
    fn the_spec_selects_virtual_devices() {
        let devices = from_spec("serial@serial:COM7; printer@usb:0416:5011 ;").expect("spec");
        let ids: Vec<DeviceId> = devices.iter().map(|d| d.id().clone()).collect();
        assert_eq!(ids, vec![id("serial:COM7"), id("usb:0416:5011")]);
        assert!(from_spec("").expect("empty").is_empty());
        assert!(from_spec("toaster@serial:COM1").is_err());
        assert!(from_spec("serial").is_err());
        assert!(from_spec("serial@nonsense").is_err());
    }
}
