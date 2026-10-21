// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Kiosk host: one full-screen window that shows `web/`, optionally driven by a signed config.
//!
//! The signed config is the pair `kiosk.json` and `kiosk.json.sig` in the kiosk folder (the
//! folder named by TPT_KIOSK_DIR, or next to the executable). It is used only when
//! TPT_KIOSK_PUBLIC_KEY holds the hex public key it was signed with. A config whose signature
//! does not check stops the app, so an edited file is never applied. Without the key the app
//! runs as an ordinary window, and says so. Signing is not in the CLI yet (see todo.md).
//!
//! Run with the path to WebView2Loader.dll as the argument, or set TPT_WEBVIEW2_LOADER.
//! Windows only: the shell runs on WebView2 today.

use std::path::{Path, PathBuf};

use tpt_shell::{embed_dir, App, Error, Window};

fn main() -> tpt_shell::Result<()> {
    let mut app = App::new()
        .name("my-kiosk")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("My Kiosk").size(1280.0, 800.0));

    match std::env::var("TPT_KIOSK_PUBLIC_KEY") {
        Ok(key) => {
            let dir = kiosk_dir();
            app = app.kiosk_config(
                &dir.join("kiosk.json"),
                &dir.join("kiosk.json.sig"),
                key.trim(),
            )?;
        }
        Err(_) => eprintln!("my-kiosk: no TPT_KIOSK_PUBLIC_KEY; running without a signed kiosk config"),
    }
    app.run()
}

fn kiosk_dir() -> PathBuf {
    std::env::var_os("TPT_KIOSK_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
        })
        .unwrap_or_default()
        .join("kiosk")
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(1))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
