// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Print jobs (spec §10.2, M7): a receipt laid out on the ESC/POS builder, a label rendered by
//! the ZPL builder, and a helper that sends bytes to a device in chunks.
//!
//! Support: Full for the layout, which is tested against golden bytes. Sending depends on the
//! device, so a printer that cannot keep up returns [`DeviceError::Busy`] after the retries.

use std::thread;
use std::time::Duration;

use crate::escpos::{Align, Receipt};
use crate::transport::{DeviceError, Transport};
use crate::{check_ascii, zpl, UnsupportedText};

/// Columns on a receipt: a common 80 mm roll in a standard font.
///
/// Support: Full.
pub const RECEIPT_COLUMNS: usize = 42;

/// Lays out a receipt: a centred bold title, one line per item with its price right-aligned,
/// a bold total, three blank lines, and a full cut. Names too long for the price column are cut.
///
/// Support: Full.
pub fn receipt(
    title: &str,
    items: &[(&str, &str)],
    total: &str,
) -> Result<Vec<u8>, UnsupportedText> {
    let mut r = Receipt::new();
    r.align(Align::Center).bold(true).line(title)?.bold(false);
    r.align(Align::Left);
    for (name, price) in items {
        check_ascii(name)?;
        check_ascii(price)?;
        let room = RECEIPT_COLUMNS.saturating_sub(price.len() + 1);
        let name: String = name.chars().take(room).collect();
        let gap = RECEIPT_COLUMNS.saturating_sub(name.len() + price.len());
        r.line(&format!("{name}{}{price}", " ".repeat(gap)))?;
    }
    check_ascii(total)?;
    r.bold(true);
    let gap = RECEIPT_COLUMNS.saturating_sub("TOTAL".len() + total.len());
    r.line(&format!("TOTAL{}{total}", " ".repeat(gap)))?;
    r.bold(false).feed(3).cut(false);
    Ok(r.into_bytes())
}

/// The bytes for one label: the ZPL block the builder renders.
///
/// Support: Full.
pub fn label(label: &zpl::Label) -> Vec<u8> {
    label.render().into_bytes()
}

/// How many times [`send_all`] retries a chunk the device refuses as busy.
const BUSY_RETRIES: u32 = 200;
/// How long to wait between retries of a busy chunk.
const BUSY_WAIT: Duration = Duration::from_millis(5);
/// Bytes per write. Small enough that a device with a modest buffer takes each chunk whole.
const CHUNK: usize = 512;

/// Sends `bytes` to the device in chunks. A chunk the device refuses as busy is retried; if it
/// stays busy, the error is returned and the chunks before it have been sent.
///
/// Support: Full.
pub fn send_all(transport: &mut dyn Transport, bytes: &[u8]) -> Result<(), DeviceError> {
    for chunk in bytes.chunks(CHUNK) {
        let mut attempts = 0;
        loop {
            match transport.write(chunk) {
                Ok(()) => break,
                Err(DeviceError::Busy) if attempts < BUSY_RETRIES => {
                    attempts += 1;
                    thread::sleep(BUSY_WAIT);
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DeviceId;
    use crate::virtual_devices::{VirtualPrinter, VirtualSerial};

    fn id(text: &str) -> DeviceId {
        text.parse().expect("id")
    }

    #[test]
    fn a_receipt_has_its_golden_layout() {
        let bytes = receipt("SHOP", &[("Tea", "2.50"), ("Scone", "4.00")], "6.50").expect("layout");
        let mut expected: Vec<u8> = vec![0x1B, b'@', 0x1B, b'a', 1, 0x1B, b'E', 1];
        expected.extend_from_slice(b"SHOP\n");
        expected.extend_from_slice(&[0x1B, b'E', 0, 0x1B, b'a', 0]);
        expected.extend_from_slice(format!("Tea{}2.50\n", " ".repeat(42 - 3 - 4)).as_bytes());
        expected.extend_from_slice(format!("Scone{}4.00\n", " ".repeat(42 - 5 - 4)).as_bytes());
        expected.extend_from_slice(&[0x1B, b'E', 1]);
        expected.extend_from_slice(format!("TOTAL{}6.50\n", " ".repeat(42 - 5 - 4)).as_bytes());
        expected.extend_from_slice(&[0x1B, b'E', 0, 0x1B, b'd', 3, 0x1D, b'V', 0]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn a_long_item_name_is_cut_before_its_price() {
        let long = "x".repeat(60);
        let bytes = receipt("T", &[(&long, "1.00")], "1.00").expect("layout");
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // The item line follows the ESC commands for alignment and emphasis, so it is found by
        // its text, not by the whole line.
        let name_cut = format!("{}{}1.00", "x".repeat(37), " ");
        assert!(text.contains(&name_cut), "name not cut to fit: {text:?}");
        assert!(
            !text.contains(&"x".repeat(38)),
            "name was not cut: {text:?}"
        );
    }

    #[test]
    fn a_label_is_the_rendered_zpl_block() {
        let mut l = zpl::Label::new();
        l.text(10, 10, 30, "Tea").expect("text");
        assert_eq!(label(&l), l.render().into_bytes());
    }

    #[test]
    fn non_ascii_in_a_receipt_is_refused() {
        assert!(receipt("CAFÉ", &[], "0").is_err());
        assert!(receipt("CAFE", &[("Tea", "2,50€")], "0").is_err());
    }

    #[test]
    fn a_receipt_is_sent_in_chunks_to_a_printer() {
        let (mut printer, output) = VirtualPrinter::new(id("usb:0416:5011"));
        let items = [("Item", "1.00"); 200];
        let big = receipt("BIG", &items, "200.00").expect("layout");
        send_all(&mut printer, &big).expect("send");
        assert_eq!(output.bytes(), big);
    }

    #[test]
    fn a_device_that_stays_busy_is_reported() {
        let mut port = VirtualSerial::loopback(id("serial:COM9"));
        port.write(&vec![0u8; 64 * 1024]).expect("fill");
        assert_eq!(send_all(&mut port, b"x"), Err(DeviceError::Busy));
    }
}
