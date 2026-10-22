// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Platform-independent rules the WKWebView layer applies (spec §9): which URLs are app
//! content, how an app path is mapped to a file, the page policy sent with every response,
//! and how a Rust string is passed into the page as a JavaScript literal. Kept free of
//! Objective-C so it is tested on every host.

use tpt_shell_core::navigation::url_is_allowlisted;

/// Origin of the app's own pages on macOS. Served by the `app` scheme handler.
///
/// Support: Full.
pub const APP_ORIGIN: &str = "app://localhost";

/// Content-Security-Policy for every response the shell serves (spec §9). Same policy as the
/// Windows backend, so the conformance suite compares like with like.
///
/// Support: Full.
pub const PAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

/// Whether `url` is under the app origin: the origin itself, or a path, query or fragment on it.
///
/// Support: Full.
pub fn is_app_url(url: &str) -> bool {
    match url.strip_prefix(APP_ORIGIN) {
        Some(rest) => rest.is_empty() || rest.starts_with(['/', '?', '#']),
        None => false,
    }
}

/// The path under the app origin that `uri` names, or `None` if it is not a safe app path. An
/// empty path or `/` means `index.html`. A query or fragment is ignored. A `..` segment or a
/// backslash is refused, so a path cannot reach outside what the app supplies.
///
/// Support: Full.
pub fn app_path(uri: &str) -> Option<String> {
    if !is_app_url(uri) {
        return None;
    }
    let rest = &uri[APP_ORIGIN.len()..];
    let path = rest
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    if path.is_empty() {
        return Some("index.html".into());
    }
    if path.contains('\\') || path.split('/').any(|segment| segment == "..") {
        return None;
    }
    Some(path.to_string())
}

/// Whether a navigation to `url` is allowed. Allowed: the app origin, `about:blank` (what a
/// page loaded with `loadHTMLString` reports as its URL), and any URL the window's allowlist
/// covers. Everything else is cancelled, including `file:` and other schemes.
///
/// Support: Full.
pub fn navigation_allowed(url: &str, allowlist: &[String]) -> bool {
    is_app_url(url) || url == "about:blank" || url_is_allowlisted(url, allowlist)
}

/// `value` as a JavaScript string literal (double-quoted), safe to splice into a script.
/// Escapes quotes, backslashes, control characters, `<` (so `</script>` cannot appear) and the
/// two line separators, which older JavaScript engines treat as line ends.
///
/// Support: Full.
pub fn js_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '<' => out.push_str("\\u003C"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The script that hands a host message to the page's `message` listeners. The bridge script
/// (see `webview::BRIDGE_JS`) defines `window.__tptDeliver`, which fires those listeners.
///
/// Support: Full.
pub fn deliver_script(message: &str) -> String {
    format!("window.__tptDeliver({});", js_string_literal(message))
}

/// Headers for a response the shell serves: the content type and the page policy.
///
/// Support: Full.
pub fn response_headers(content_type: &str) -> [(&'static str, String); 2] {
    [
        ("Content-Type", content_type.to_string()),
        ("Content-Security-Policy", PAGE_CSP.to_string()),
    ]
}

/// File extensions named by (display name, pattern) dialog filters. `*.txt` gives `txt`, and
/// several patterns can be joined with `;`. A pattern that matches everything (`*`, `*.*`)
/// turns the filter off and gives an empty list, because the panels take extensions only.
///
/// Support: Full.
pub fn file_extensions(filters: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, pattern) in filters {
        for part in pattern.split(';') {
            let part = part.trim();
            let ext = part.strip_prefix("*.").unwrap_or(part);
            if ext.is_empty() || ext == "*" {
                return Vec::new();
            }
            if !out.iter().any(|e| e == ext) {
                out.push(ext.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_urls_match_the_origin_at_a_boundary() {
        assert!(is_app_url("app://localhost"));
        assert!(is_app_url("app://localhost/"));
        assert!(is_app_url("app://localhost/a/b.js?x#y"));
        assert!(!is_app_url("app://localhost.evil.net/"));
        assert!(!is_app_url("app://localhostx/"));
        assert!(!is_app_url("https://localhost/"));
    }

    #[test]
    fn app_paths_default_to_index_and_refuse_traversal() {
        assert_eq!(app_path("app://localhost").as_deref(), Some("index.html"));
        assert_eq!(app_path("app://localhost/").as_deref(), Some("index.html"));
        assert_eq!(
            app_path("app://localhost/js/app.js?v=2").as_deref(),
            Some("js/app.js")
        );
        assert_eq!(app_path("app://localhost/../secret"), None);
        assert_eq!(app_path("app://localhost/a\\b"), None);
        assert_eq!(app_path("https://example.com/"), None);
    }

    #[test]
    fn navigation_allows_app_pages_blank_and_the_allowlist_only() {
        let allow = vec!["https://docs.example.com/".to_string()];
        assert!(navigation_allowed("app://localhost/x", &allow));
        assert!(navigation_allowed("about:blank", &allow));
        assert!(navigation_allowed("https://docs.example.com/a", &allow));
        assert!(!navigation_allowed("https://evil.example.com/", &allow));
        assert!(!navigation_allowed("file:///etc/passwd", &allow));
        assert!(!navigation_allowed("javascript:alert(1)", &allow));
        assert!(!navigation_allowed("https://example.com", &[]));
    }

    #[test]
    fn js_literals_cannot_break_out_of_the_script() {
        assert_eq!(js_string_literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string_literal("x\\y"), "\"x\\\\y\"");
        assert_eq!(js_string_literal("</script>"), "\"\\u003C/script>\"");
        assert_eq!(js_string_literal("\n\r\t"), "\"\\n\\r\\t\"");
        assert_eq!(js_string_literal("\u{1}"), "\"\\u0001\"");
        assert_eq!(js_string_literal("\u{2028}"), "\"\\u2028\"");
        assert_eq!(js_string_literal("é"), "\"é\"");
    }

    #[test]
    fn deliver_script_wraps_the_message_as_one_argument() {
        assert_eq!(
            deliver_script("{\"v\":1}"),
            "window.__tptDeliver(\"{\\\"v\\\":1}\");"
        );
    }

    #[test]
    fn every_response_carries_the_page_policy_without_inline_script() {
        let headers = response_headers("text/html");
        assert_eq!(headers[0].1, "text/html");
        assert_eq!(headers[1].0, "Content-Security-Policy");
        assert!(headers[1].1.contains("script-src 'self'"));
        assert!(!headers[1].1.contains("unsafe-inline"));
    }

    fn filter(pattern: &str) -> Vec<(String, String)> {
        vec![("Files".to_string(), pattern.to_string())]
    }

    #[test]
    fn dialog_patterns_become_extensions() {
        assert_eq!(file_extensions(&filter("*.txt")), vec!["txt"]);
        assert_eq!(file_extensions(&filter("*.png;*.jpg")), vec!["png", "jpg"]);
    }

    #[test]
    fn a_match_all_dialog_pattern_removes_the_restriction() {
        assert!(file_extensions(&filter("*.*")).is_empty());
        assert!(file_extensions(&[]).is_empty());
    }
}
