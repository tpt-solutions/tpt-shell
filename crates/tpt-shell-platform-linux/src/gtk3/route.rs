// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Pure rules for the WebKit side: which URLs are app pages, how an `app://` URL maps to an
//! asset key, which navigations the policy allows, and how a page message is quoted into a
//! script. Nothing here calls GTK, so these rules are tested on every platform.

/// The origin the app's pages are served from. The blob route is `app://__tpt/blob/<id>`
/// (the `__tpt` host), which [`asset_key`] maps to `__tpt/blob/<id>`.
pub const APP_ORIGIN: &str = "app://tpt-app";

/// The host that names the blob channel.
pub const BLOB_HOST: &str = "__tpt";

/// Page script the shell injects before any page script runs. It provides the
/// `window.chrome.webview` surface the JavaScript runtime expects, backed by the WebKit message
/// handler named `tptShell`.
pub const PAGE_POLYFILL: &str = r#"(function () {
  if (window.chrome && window.chrome.webview) { return; }
  window.chrome = window.chrome || {};
  window.chrome.webview = {
    postMessage: function (message) {
      window.webkit.messageHandlers.tptShell.postMessage(String(message));
    },
    addEventListener: function (type, listener) {
      window.addEventListener(type, listener);
    },
    removeEventListener: function (type, listener) {
      window.removeEventListener(type, listener);
    }
  };
})();"#;

/// The name of the script message handler the page posts to.
pub const MESSAGE_HANDLER: &str = "tptShell";

/// True for URLs on the app origin, such as `app://tpt-app/index.html`.
pub fn is_app_uri(uri: &str) -> bool {
    match uri.strip_prefix(APP_ORIGIN) {
        Some(rest) => {
            rest.is_empty()
                || rest.starts_with('/')
                || rest.starts_with('?')
                || rest.starts_with('#')
        }
        None => false,
    }
}

/// Maps an `app://` URL to the asset key the app's asset source knows.
///
/// `app://tpt-app/` and `app://tpt-app/index.html` map to `index.html`. The blob host maps as
/// `app://__tpt/blob/<id>` to `__tpt/blob/<id>`. A key with `..`, a backslash, a NUL or an
/// empty segment is refused (`None`), so a request cannot name a path outside the app.
pub fn asset_key(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("app://")?;
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let decoded = percent_decode(path)?;
    if decoded.contains('\\') || decoded.contains('\0') {
        return None;
    }
    let segments: Vec<&str> = decoded.split('/').collect();
    if segments.contains(&"..") {
        return None;
    }
    let key = if host == BLOB_HOST {
        if decoded.is_empty() {
            return None;
        }
        format!("{BLOB_HOST}/{decoded}")
    } else if decoded.is_empty() {
        "index.html".to_string()
    } else {
        if segments.iter().any(|s| s.is_empty()) {
            return None;
        }
        decoded
    };
    Some(key)
}

/// Whether a navigation may proceed. The app origin and `about:blank` always may. Anything else
/// needs a prefix from the window's allowlist, matched at a host boundary: the prefix
/// `https://example.com` admits `https://example.com/x` but not `https://example.com.evil.net`.
pub fn navigation_allowed(uri: &str, allow: &[String]) -> bool {
    if is_app_uri(uri) || uri == "about:blank" {
        return true;
    }
    allow.iter().any(|prefix| prefix_matches(uri, prefix))
}

/// Host-boundary prefix match. Only `https://` prefixes are honoured, so an allowlist entry can
/// never admit a `file:`, `data:` or `javascript:` URL.
pub fn prefix_matches(uri: &str, prefix: &str) -> bool {
    if !prefix.starts_with("https://") || prefix.len() <= "https://".len() {
        return false;
    }
    let Some(rest) = uri.strip_prefix(prefix) else {
        return false;
    };
    prefix.ends_with('/') || rest.is_empty() || rest.starts_with(['/', '?', '#'])
}

/// Quotes `text` as a JavaScript string literal. The result can be placed in a script without
/// the text being able to end the literal or the script.
pub fn js_string_literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The script that delivers `message` to the page's `message` listeners, as a `MessageEvent`
/// (the same event the Windows backend produces with `PostWebMessageAsString`).
pub fn dispatch_script(message: &str) -> String {
    format!(
        "window.dispatchEvent(new MessageEvent('message', {{ data: {} }}));",
        js_string_literal(message)
    )
}

/// Decodes `%XX` escapes in a path. `None` for a malformed escape or bytes that are not UTF-8.
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hi = (hex[0] as char).to_digit(16)?;
            let lo = (hex[1] as char).to_digit(16)?;
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A file-dialog filter pattern: `*.txt`, `report-??.pdf` and the like. Only letters, digits,
/// `*`, `?`, `.`, `_` and `-` are allowed, so a pattern cannot carry anything but a name match.
pub fn valid_glob(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= 64
        && pattern
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '*' | '?' | '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_uris_are_recognised_by_origin_not_by_prefix() {
        assert!(is_app_uri("app://tpt-app/index.html"));
        assert!(is_app_uri("app://tpt-app"));
        assert!(is_app_uri("app://tpt-app?x=1"));
        assert!(
            !is_app_uri("app://tpt-app.evil/x"),
            "a longer host is not the app origin"
        );
        assert!(!is_app_uri("https://tpt-app/x"));
    }

    #[test]
    fn asset_keys_map_pages_and_the_blob_channel() {
        assert_eq!(asset_key("app://tpt-app/").as_deref(), Some("index.html"));
        assert_eq!(
            asset_key("app://tpt-app/index.html").as_deref(),
            Some("index.html")
        );
        assert_eq!(
            asset_key("app://tpt-app/app.js?v=2").as_deref(),
            Some("app.js")
        );
        assert_eq!(
            asset_key("app://tpt-app/a%20b.css").as_deref(),
            Some("a b.css")
        );
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            asset_key(&format!("app://__tpt/blob/{id}")).as_deref(),
            Some(format!("__tpt/blob/{id}").as_str())
        );
        assert_eq!(
            asset_key(&format!("app://tpt-app/__tpt/blob/{id}")).as_deref(),
            Some(format!("__tpt/blob/{id}").as_str())
        );
    }

    #[test]
    fn asset_keys_refuse_traversal_and_malformed_paths() {
        assert_eq!(asset_key("app://tpt-app/../secret"), None);
        assert_eq!(asset_key("app://tpt-app/a/..%2f..%2fx"), None);
        assert_eq!(
            asset_key("app://tpt-app/a%5c..%5cb"),
            None,
            "a backslash is refused"
        );
        assert_eq!(asset_key("app://tpt-app/a%00b"), None, "a NUL is refused");
        assert_eq!(
            asset_key("app://tpt-app/a%zz"),
            None,
            "a bad escape is refused"
        );
        assert_eq!(asset_key("app://tpt-app//double"), None);
        assert_eq!(asset_key("app://__tpt/"), None, "the blob host needs an id");
        assert_eq!(
            asset_key("https://tpt-app/x"),
            None,
            "only the app scheme maps"
        );
    }

    #[test]
    fn navigation_admits_the_app_and_allowlisted_https_only() {
        let allow = vec!["https://docs.example.com".to_string()];
        assert!(navigation_allowed("app://tpt-app/x.html", &allow));
        assert!(navigation_allowed("about:blank", &allow));
        assert!(navigation_allowed("https://docs.example.com/page", &allow));
        assert!(navigation_allowed("https://docs.example.com", &allow));
        assert!(
            !navigation_allowed("https://docs.example.com.evil.net/", &allow),
            "host boundary"
        );
        assert!(!navigation_allowed("https://other.example.com/", &allow));
        assert!(!navigation_allowed("file:///etc/passwd", &allow));
        assert!(!navigation_allowed("data:text/html,x", &allow));
        assert!(!navigation_allowed("javascript:alert(1)", &allow));
        assert!(
            !navigation_allowed("https://docs.example.com/", &[]),
            "an empty list admits no remote page"
        );
    }

    #[test]
    fn a_prefix_must_be_https_and_name_a_host() {
        assert!(!prefix_matches("http://a.test/x", "http://a.test"));
        assert!(!prefix_matches("https://a.test/x", "https://"));
        assert!(prefix_matches("https://a.test/x", "https://a.test/"));
        assert!(prefix_matches("https://a.test?q", "https://a.test"));
    }

    #[test]
    fn js_literals_cannot_end_the_string_or_the_script() {
        assert_eq!(js_string_literal("a\"b"), r#""a\"b""#);
        assert_eq!(js_string_literal("x\\y"), r#""x\\y""#);
        let hostile = "</script><script>alert(1)</script>\u{2028}";
        let lit = js_string_literal(hostile);
        assert!(!lit.contains("</script>"));
        assert!(!lit.contains('\u{2028}'));
        assert!(lit.starts_with('"') && lit.ends_with('"'));
        assert!(!js_string_literal("a\u{1}b").contains('\u{1}'));
    }

    #[test]
    fn dispatch_script_wraps_the_message_as_data() {
        let script = dispatch_script("hi \"there\"");
        assert!(script.starts_with("window.dispatchEvent(new MessageEvent('message', { data: \""));
        assert!(script.contains("hi \\\"there\\\""));
    }

    #[test]
    fn percent_decoding_is_strict() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("%e2%82%ac").as_deref(), Some("€"));
        assert_eq!(percent_decode("%ff"), None, "invalid UTF-8 is refused");
        assert_eq!(percent_decode("%4"), None);
    }

    #[test]
    fn filter_patterns_are_plain_names() {
        assert!(valid_glob("*.txt"));
        assert!(valid_glob("report-??.pdf"));
        assert!(!valid_glob("*.txt;rm"));
        assert!(!valid_glob("a/b"));
        assert!(!valid_glob(""));
    }

    #[test]
    fn the_polyfill_only_fills_in_what_is_missing() {
        assert!(PAGE_POLYFILL.contains("window.chrome.webview"));
        assert!(PAGE_POLYFILL.contains("window.webkit.messageHandlers.tptShell.postMessage"));
        assert!(PAGE_POLYFILL.contains("return;"));
    }
}
