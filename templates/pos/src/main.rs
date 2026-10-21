// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Point-of-sale host: a till window whose page prints receipts to the default printer.
//!
//! The page sends the items as whole cents. The host checks them, totals them and lays out the
//! receipt (ESC/POS bytes from `devices::jobs::receipt`), then sends it to the system's default
//! printer through the OS spooler. Printing needs the `device.print` grant, which asks the
//! operator once per window in a native dialog (default No).
//!
//! Run with the path to WebView2Loader.dll as the argument, or set TPT_WEBVIEW2_LOADER.
//! Windows only for now: the shell runs on WebView2, and the spooler is Partial on other OSes.

use std::path::PathBuf;

use serde_json::{json, Value};
use tpt_shell::devices::{jobs, spool};
use tpt_shell::{embed_dir, App, CommandError, Error, Grant, Scope, Target, Window};

/// The origin the window's page is served from. It is fixed for every app.
const ORIGIN: &str = "https://tpt-app.localhost";

const MAX_ITEMS: usize = 50;
const MAX_NAME_CHARS: usize = 24;
const MAX_CENTS: u64 = 1_000_000;

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("my-pos")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("Till").size(1024.0, 700.0))
        .grant(Grant {
            capability: "device.print".into(),
            window: "main".into(),
            origin: ORIGIN.into(),
            scope: Scope::Unrestricted,
            consent: true,
        })
        .command(
            "receipt.print",
            "device.print",
            |_| Target::None,
            print_receipt,
        )
        .run()
}

/// Checks the items, totals them, and prints the receipt on the default printer.
fn print_receipt(args: &Value) -> Result<Value, CommandError> {
    let items = args["items"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= MAX_ITEMS)
        .ok_or_else(|| CommandError::new("POS_BAD_ITEMS", "send 1 to 50 items"))?;

    let mut lines: Vec<(String, String)> = Vec::with_capacity(items.len());
    let mut total: u64 = 0;
    for item in items {
        let name = item["name"]
            .as_str()
            .filter(|n| !n.is_empty() && n.chars().count() <= MAX_NAME_CHARS)
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

    let printer = spool::default_printer()
        .ok_or_else(|| CommandError::new("POS_NO_PRINTER", "no default printer is set"))?;
    spool::send_raw(&printer, "receipt", &bytes)
        .map_err(|e| CommandError::new("POS_PRINT_FAILED", &e.to_string()))?;
    Ok(json!({ "printed": rows.len(), "total": money(total) }))
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
