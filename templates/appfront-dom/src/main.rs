// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The host for the DOM front end in `web/`: one window, and the commands its page calls.
//!
//! Run with the path to WebView2Loader.dll as the argument, or set TPT_WEBVIEW2_LOADER.
//! Windows only: the shell runs on WebView2 today.

use std::path::PathBuf;

use serde_json::json;
use tpt_shell::{embed_dir, App, CommandError, Error, Value, Window};

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("my-app")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("My App").size(960.0, 640.0))
        .register(|router| {
            // The page calls this as window.tpt.invoke("app.info").
            router.register("app.info", |_: &Value| -> Result<Value, CommandError> {
                Ok(json!({ "name": "my-app", "version": env!("CARGO_PKG_VERSION") }))
            });
        })
        .run()
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(1))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
