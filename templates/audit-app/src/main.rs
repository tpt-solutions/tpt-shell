// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Audited app: a notes window whose every command call is written to `audit.jsonl`.
//!
//! The log policy comes from the `[audit]` section of tpt-shell.toml: parameters are not
//! stored, only their hash, and the redacted names are never kept. Check the log with
//! `tpt-shell audit verify audit.jsonl` and read it with `tpt-shell audit view audit.jsonl`.
//!
//! The manifest is read from this project's folder (CARGO_MANIFEST_DIR), which is right for
//! `cargo run`. A shipped app must put tpt-shell.toml next to its executable instead.
//!
//! Run with the path to WebView2Loader.dll as the argument, or set TPT_WEBVIEW2_LOADER.
//! Windows only: the shell runs on WebView2 today.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{json, Value};
use tpt_shell::{embed_dir, App, CommandError, Error, Window};

fn main() -> tpt_shell::Result<()> {
    let notes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let for_add = notes.clone();
    let for_list = notes.clone();

    let app = App::new()
        .name("my-audit-app")
        .loader(loader_path()?)
        .assets(embed_dir!("web"))
        .window(Window::new("main").title("Notes (audited)").size(800.0, 560.0))
        .register(move |router| {
            router.register("notes.add", move |args: &Value| -> Result<Value, CommandError> {
                let text = args["text"]
                    .as_str()
                    .filter(|t| !t.is_empty() && t.chars().count() <= 200)
                    .ok_or_else(|| CommandError::new("NOTES_BAD_TEXT", "text of 1 to 200 characters"))?;
                for_add.borrow_mut().push(text.to_string());
                Ok(json!({ "count": for_add.borrow().len() }))
            });
            router.register("notes.list", move |_: &Value| -> Result<Value, CommandError> {
                Ok(json!({ "notes": *for_list.borrow() }))
            });
        });

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tpt-shell.toml");
    app.manifest_audit(&manifest, "audit.jsonl")?.run()
}

fn loader_path() -> tpt_shell::Result<PathBuf> {
    std::env::var_os("TPT_WEBVIEW2_LOADER")
        .or_else(|| std::env::args_os().nth(1))
        .map(PathBuf::from)
        .ok_or(Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })
}
