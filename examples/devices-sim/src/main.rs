// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Devices without hardware: a receipt printer, a barcode scanner, a HID device and a serial
//! loopback, all virtual, behind one device hub. The program prints a receipt preview, scans
//! two barcodes, exchanges HID reports and echoes bytes on the serial port, then shows the audit
//! trail the hub wrote for each open, write and close.
//!
//! No window and no WebView2: this runs the same on every OS (`cargo run -p devices-sim`).
//! Use it in tests and in front-end work where the real devices are not on the desk.

use std::fmt::Display;
use std::sync::{Arc, Mutex};

use tpt_shell::devices::hub::{DeviceHub, Event};
use tpt_shell::devices::id::{DeviceId, DeviceScope};
use tpt_shell::devices::jobs;
use tpt_shell::devices::scanner::ScanFramer;
use tpt_shell::devices::transport::Transport;
use tpt_shell::devices::virtual_devices::{VirtualHid, VirtualPrinter, VirtualScanner, VirtualSerial};

fn main() -> Result<(), String> {
    let printer_id: DeviceId = "usb:0416:5011".parse().map_err(text)?;
    let scanner_id: DeviceId = "hid:05f9:ff01".parse().map_err(text)?;
    let hid_id: DeviceId = "hid:046d:c52b".parse().map_err(text)?;
    let serial_id: DeviceId = "serial:COM3".parse().map_err(text)?;

    // Every device is a Transport. The hub hands each one out once, and audits its use.
    let (printer, printed) = VirtualPrinter::new(printer_id.clone());
    let (scanner, scans) = VirtualScanner::new(scanner_id.clone());
    let (hid, hid_handle) = VirtualHid::new(hid_id.clone());
    let serial = VirtualSerial::loopback(serial_id.clone());

    let audit: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = audit.clone();
    let devices: Vec<Box<dyn Transport>> = vec![
        Box::new(printer),
        Box::new(scanner),
        Box::new(hid),
        Box::new(serial),
    ];
    let mut hub = DeviceHub::new(devices, move |event: &Event| {
        sink.lock()
            .expect("audit lock")
            .push(format!("{:?} {} {} bytes", event.kind, event.device, event.bytes));
    });

    // 1. Receipt: lay it out, print it, and show the preview the printer kept.
    let receipt = jobs::receipt(
        "SHOP",
        &[("Flat white", "4.50"), ("Muffin", "5.20")],
        "9.70",
    )
    .map_err(text)?;
    let printer_scope = DeviceScope::Usb {
        vendor: Some(0x0416),
        product: Some(0x5011),
    };
    let mut open = hub.open(&printer_id, &printer_scope).map_err(text)?;
    open.write(&receipt).map_err(text)?;
    drop(open);
    println!("--- receipt preview ---\n{}", printed.preview());

    // 2. Barcode scanner: two scans, read back and split into codes.
    scans.scan("9421903110012");
    scans.scan("ABC-123");
    let scanner_scope = DeviceScope::Hid {
        vendor: Some(0x05f9),
        product: Some(0xff01),
    };
    let codes = hub.list(&scanner_scope);
    println!("scanner devices in scope: {codes:?}");
    let mut open = hub.open(&scanner_id, &scanner_scope).map_err(text)?;
    let bytes = open.read(256).map_err(text)?;
    drop(open);
    let mut framer = ScanFramer::new();
    for result in framer.push(&bytes) {
        match result {
            Ok(code) => println!("scan: {code}"),
            Err(e) => println!("scan refused: {e:?}"),
        }
    }

    // 3. HID: one input report in, one output report out.
    hid_handle.send_report(&[0x01, 0x00, 0x2a]);
    let hid_scope = DeviceScope::Hid {
        vendor: Some(0x046d),
        product: Some(0xc52b),
    };
    let mut open = hub.open(&hid_id, &hid_scope).map_err(text)?;
    let report = open.read(8).map_err(text)?;
    open.write(&[0xAA, 0x55]).map_err(text)?;
    drop(open);
    println!("hid input report: {report:02x?}; output reports: {:02x?}", hid_handle.output_reports());

    // 4. Serial loopback: what goes out comes back.
    let serial_scope = DeviceScope::Serial {
        port: "COM*".into(),
    };
    let mut open = hub.open(&serial_id, &serial_scope).map_err(text)?;
    open.write(b"ping").map_err(text)?;
    let echoed = open.read(4).map_err(text)?;
    drop(open);
    println!("serial echo: {}", String::from_utf8_lossy(&echoed));

    // 5. The audit trail: every open, write and close, with byte counts. Reads are not audited.
    println!("--- audit ---");
    for line in audit.lock().expect("audit lock").iter() {
        println!("{line}");
    }
    Ok(())
}

fn text(e: impl Display) -> String {
    e.to_string()
}
