// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Till with a granted receipt printer, and no printer needed: the printer is virtual.
//!
//! The page sends its items in cents. The host checks them, totals them, lays out the receipt,
//! and writes it to a virtual ESC/POS printer through the device hub. The reply carries the
//! receipt preview and the hub's audit lines, so the page can show both.
//!
//! The grant is `device.print` for the printer `usb:0416:5011` only. A page asking for any
//! other device is refused before this code runs. A device opens once per hub, so each print
//! builds a fresh hub and a fresh virtual printer.
//!
//! Set TPT_WEBVIEW2_LOADER to WebView2Loader.dll (or pass its path as the first argument).
//! Windows only: the shell runs on WebView2 today.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tpt_shell::devices::hub::{DeviceHub, Event};
use tpt_shell::devices::id::{DeviceId, DeviceScope};
use tpt_shell::devices::jobs;
use tpt_shell::devices::transport::Transport;
use tpt_shell::devices::virtual_devices::VirtualPrinter;
use tpt_shell::{embed_dir, App, CommandError, Error, Grant, Scope, Target, Window};

const ORIGIN: &str = "https://tpt-app.localhost";
const PRINTER: &str = "usb:0416:5011";
const MAX_ITEMS: usize = 50;
const MAX_CENTS: u64 = 1_000_000;

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("pos-example")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("Till (virtual printer)").size(1024.0, 700.0))
        .grant(Grant {
            capability: "device.print".into(),
            window: "main".into(),
            origin: ORIGIN.into(),
            scope: Scope::Devices(vec![PRINTER.into()]),
            consent: false,
        })
        .command(
            "receipt.print",
            "device.print",
            |args| match args["device"].as_str() {
                Some(device) => Target::Device(device.to_string()),
                None => Target::None,
            },
            print_receipt,
        )
        .run()
}

fn print_receipt(args: &Value) -> Result<Value, CommandError> {
    let items = args["items"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= MAX_ITEMS)
        .ok_or_else(|| CommandError::new("POS_BAD_ITEMS", "send 1 to 50 items"))?;

    let mut lines: Vec<(String, String)> = Vec::new();
    let mut total: u64 = 0;
    for item in items {
        let name = item["name"]
            .as_str()
            .filter(|n| !n.is_empty() && n.chars().count() <= 24)
            .ok_or_else(|| CommandError::new("POS_BAD_ITEM", "a name of 1 to 24 characters"))?;
        let cents = item["cents"]
            .as_u64()
            .filter(|c| *c <= MAX_CENTS)
            .ok_or_else(|| CommandError::new("POS_BAD_ITEM", "cents from 0 to 1000000"))?;
        total += cents;
        lines.push((name.to_string(), money(cents)));
    }
    let rows: Vec<(&str, &str)> = lines
        .iter()
        .map(|(name, price)| (name.as_str(), price.as_str()))
        .collect();
    let bytes = jobs::receipt("RECEIPT", &rows, &money(total))
        .map_err(|e| CommandError::new("POS_BAD_TEXT", &e.to_string()))?;

    let printer_id: DeviceId = PRINTER
        .parse()
        .map_err(|e: tpt_shell::devices::id::ParseError| CommandError::new("POS_DEVICE", &e.0))?;
    let (printer, output) = VirtualPrinter::new(printer_id.clone());
    let audit: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = audit.clone();
    let mut hub = DeviceHub::new(
        vec![Box::new(printer) as Box<dyn Transport>],
        move |event: &Event| {
            sink.lock()
                .expect("audit lock")
                .push(format!("{:?} {} {} bytes", event.kind, event.device, event.bytes));
        },
    );
    let scope = DeviceScope::Usb {
        vendor: Some(0x0416),
        product: Some(0x5011),
    };
    let mut open = hub
        .open(&printer_id, &scope)
        .map_err(|e| CommandError::new("POS_DEVICE_OPEN", &e.to_string()))?;
    open.write(&bytes)
        .map_err(|e| CommandError::new("POS_DEVICE_WRITE", &e.to_string()))?;
    drop(open);

    let audit_lines = audit.lock().expect("audit lock").clone();
    Ok(json!({
        "printed": rows.len(),
        "total": money(total),
        "preview": output.preview(),
        "audit": audit_lines,
    }))
}

/// Whole cents as `12.34`.
fn money(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(1))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
