// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The smallest app: one window showing one embedded page. Used for the hello-world size
//! budget (spec §13). Windows only.
//!
//! Only one copy runs at a time: a later launch passes its arguments to the first and exits.
//!
//! usage: cargo run -p tpt-shell --example hello -- <path to WebView2Loader.dll>

use tpt_shell::{App, Window};

const PAGE: &str = "<!doctype html><title>hello</title><h1>hello from tpt-shell</h1>";

fn main() -> tpt_shell::Result<()> {
    let loader = std::env::args()
        .nth(1)
        .ok_or(tpt_shell::Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })?;
    App::new()
        .name("hello")
        .loader(loader)
        .single_instance("hello", |args| eprintln!("second launch: {args:?}"))
        .asset("index.html", "text/html", PAGE)
        .window(Window::new("main").title("hello"))
        .run()
}
