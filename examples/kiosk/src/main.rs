// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Kiosk window with a signed local config: the home page and the idle reset come from the
//! config, and the app applies it only if its Ed25519 signature checks out.
//!
//! usage: kiosk [--config <kiosk.json> --sig <kiosk.json.sig> --key <public key, hex>]
//!
//! Without the three arguments the window runs as an ordinary window and says so. With them, a
//! config whose signature does not check stops the app at start (`App::kiosk_config` fails).
//! The config is JSON with `home` (a path under the app, default index.html) and `idle_seconds`
//! (optional). Making the signature needs the signing key; the CLI does not sign yet (todo.md).
//!
//! Set TPT_WEBVIEW2_LOADER to WebView2Loader.dll (or pass its path as the first argument).
//! Windows only: the shell runs on WebView2 today.

use std::path::PathBuf;

use tpt_shell::{embed_dir, App, Error, Window};

fn main() -> tpt_shell::Result<()> {
    let mut app = App::new()
        .name("kiosk-example")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("Kiosk").size(1280.0, 800.0));

    match kiosk_args(std::env::args().skip(1).collect())? {
        Some((config, sig, key)) => {
            app = app.kiosk_config(&config, &sig, &key)?;
        }
        None => eprintln!("kiosk: no signed config given; running as an ordinary window"),
    }
    app.run()
}

/// Reads `--config`, `--sig` and `--key`. All three or none.
fn kiosk_args(args: Vec<String>) -> tpt_shell::Result<Option<(PathBuf, PathBuf, String)>> {
    let mut config = None;
    let mut sig = None;
    let mut key = None;
    let mut rest = args.into_iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or(Error::Unsupported {
            feature: "a kiosk flag without its value",
        })?;
        match flag.as_str() {
            "--config" => config = Some(PathBuf::from(value)),
            "--sig" => sig = Some(PathBuf::from(value)),
            "--key" => key = Some(value),
            _ => {
                return Err(Error::Unsupported {
                    feature: "an unknown kiosk flag (use --config, --sig and --key)",
                })
            }
        }
    }
    match (config, sig, key) {
        (None, None, None) => Ok(None),
        (Some(config), Some(sig), Some(key)) => Ok(Some((config, sig, key))),
        _ => Err(Error::Unsupported {
            feature: "a kiosk config needs --config, --sig and --key together",
        }),
    }
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(1).filter(|a| a.to_str() != Some("--config")))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
