// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Procedural macros for tpt-shell (spec §7.1, §2).
//!
//! `embed_dir!("dir")` embeds every file under `dir` (relative to the calling crate's
//! `Cargo.toml`) at compile time. It expands to `&[(path, content_type, bytes)]`, with paths
//! using `/` and sorted, so the output is deterministic.
//!
//! Limitation: Cargo does not rerun the build when a file is added to the directory, only when
//! an embedded file changes. Touch a source file after adding one.

mod command;

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, LitStr};

#[proc_macro]
pub fn embed_dir(input: TokenStream) -> TokenStream {
    let dir = parse_macro_input!(input as LitStr);
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default();
    let root = manifest.join(dir.value());

    let mut files = Vec::new();
    if let Err(err) = collect(&root, &root, &mut files) {
        let msg = format!("embed_dir!: cannot read {}: {err}", root.display());
        return syn::Error::new(dir.span(), msg).to_compile_error().into();
    }
    files.sort();

    let entries = files.iter().map(|(rel, abs)| {
        let abs = abs.to_string_lossy().replace('\\', "/");
        let mime = content_type(rel);
        quote! { (#rel, #mime, include_bytes!(#abs) as &[u8]) }
    });
    quote! { &[ #(#entries),* ] as &[(&str, &str, &[u8])] }.into()
}

/// Walks `dir`, pushing (path relative to `root` with `/` separators, absolute path).
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// Content type from the file extension. Unknown extensions get `application/octet-stream`.
fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Marks a function as a command: `#[command(capability = "fs.read", name = "list_dir")]`.
///
/// The function itself is unchanged. Hidden helpers are added beside it; see [`commands!`]
/// and [`register_commands!`]. Options: `name` (defaults to the function name), `capability`,
/// `target(path = "arg")` (or `device`, `url`, `bytes`: the argument the grant's scope checks,
/// which needs a capability), and `kind = "sync"` (the only kind so far).
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemFn);
    match command::expand_command(attr.into(), item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// `commands![a, b]` builds a `Contract` from `#[command]` functions in the same module.
#[proc_macro]
pub fn commands(input: TokenStream) -> TokenStream {
    match command::expand_commands(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// `register_commands!(&mut router, a, b)` registers each `#[command]` handler on the router.
#[proc_macro]
pub fn register_commands(input: TokenStream) -> TokenStream {
    match command::expand_register(input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::content_type;

    #[test]
    fn content_types_by_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("js/app.JS"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("logo.svg"), "image/svg+xml");
        assert_eq!(content_type("data.bin"), "application/octet-stream");
        assert_eq!(content_type("README"), "application/octet-stream");
    }
}
