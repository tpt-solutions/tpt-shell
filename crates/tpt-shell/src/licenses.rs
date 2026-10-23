// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The licence page, served at `app://__tpt/licenses` (spec §10.8). The page is the compliance
//! output that `build.rs` finds at build time. A build without that file serves a short notice
//! rather than an error.

use tpt_shell_core::backend::Asset;

#[cfg_attr(not(windows), allow(unused_imports))]
pub use tpt_shell_compliance::notices::LICENSES_PATH;

#[cfg(has_licenses)]
const EMBEDDED: Option<&str> = Some(include_str!(env!("TPT_SHELL_LICENSES_FILE")));
#[cfg(not(has_licenses))]
const EMBEDDED: Option<&str> = None;

const NOTICE: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Licences</title>\n\
</head>\n<body>\n<p>The licence notices for this build are not included. They are produced by the \
compliance step and embedded when the app is built.</p>\n</body>\n</html>\n";

/// The licence page as an asset of the app origin.
///
/// Support: Partial. Shows a notice unless the compliance step embeds the licence page at build
/// time.
pub fn asset() -> Asset {
    Asset {
        content_type: "text/html; charset=utf-8".into(),
        body: EMBEDDED.unwrap_or(NOTICE).as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_licence_page_is_html_and_never_empty() {
        let page = asset();
        assert_eq!(page.content_type, "text/html; charset=utf-8");
        assert!(!page.body.is_empty());
        assert_eq!(LICENSES_PATH, "__tpt/licenses");
    }
}
