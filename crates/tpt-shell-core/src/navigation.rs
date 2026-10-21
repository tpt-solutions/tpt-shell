// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Per-window navigation allowlists (spec §9): remote pages load only when a window lists
//! them. An entry is an `https://` URL prefix. Backends check navigations against it; the
//! manifest checks entries when it parses them.

/// Whether `entry` is a valid allowlist entry: an `https://` URL with a host, no whitespace,
/// and no user information (`user@host`), which would let a prefix match a different host.
pub fn is_valid_entry(entry: &str) -> bool {
    let Some(rest) = entry.strip_prefix("https://") else {
        return false;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    !authority.is_empty() && !authority.contains('@') && !entry.chars().any(char::is_whitespace)
}

/// Whether `url` falls under `entry`. The match stops at a boundary: `https://a.com` allows
/// `https://a.com/x` and `https://a.com?q`, but not `https://a.com.evil.com` or
/// `https://a.com@evil.com`. An entry ending in `/` allows everything under that path.
pub fn url_matches_entry(url: &str, entry: &str) -> bool {
    if !is_valid_entry(entry) || !url.starts_with(entry) {
        return false;
    }
    if entry.ends_with('/') {
        return true;
    }
    matches!(
        url[entry.len()..].chars().next(),
        None | Some('/' | '?' | '#')
    )
}

/// Whether any entry in `allowlist` covers `url`.
pub fn url_is_allowlisted(url: &str, allowlist: &[String]) -> bool {
    allowlist.iter().any(|entry| url_matches_entry(url, entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_with_a_host_are_valid_entries() {
        assert!(is_valid_entry("https://example.com"));
        assert!(is_valid_entry("https://example.com/docs/"));
        assert!(!is_valid_entry("http://example.com/"));
        assert!(!is_valid_entry("https://"));
        assert!(!is_valid_entry("https:///path"));
        assert!(!is_valid_entry("https://user@example.com/"));
        assert!(!is_valid_entry("https://example.com/a b"));
        assert!(!is_valid_entry("example.com"));
    }

    #[test]
    fn an_entry_allows_its_own_url_and_paths_under_it() {
        assert!(url_matches_entry(
            "https://example.com",
            "https://example.com"
        ));
        assert!(url_matches_entry(
            "https://example.com/",
            "https://example.com"
        ));
        assert!(url_matches_entry(
            "https://example.com/a?b#c",
            "https://example.com"
        ));
        assert!(url_matches_entry(
            "https://example.com?q=1",
            "https://example.com"
        ));
    }

    #[test]
    fn a_host_prefix_does_not_match_a_longer_host_or_user_info() {
        assert!(!url_matches_entry(
            "https://example.com.evil.net/",
            "https://example.com"
        ));
        assert!(!url_matches_entry(
            "https://example.company/",
            "https://example.com"
        ));
        assert!(!url_matches_entry(
            "https://example.com@evil.net/",
            "https://example.com"
        ));
    }

    #[test]
    fn a_trailing_slash_entry_covers_its_path_only() {
        assert!(url_matches_entry(
            "https://example.com/docs/x",
            "https://example.com/docs/"
        ));
        assert!(!url_matches_entry(
            "https://example.com/other",
            "https://example.com/docs/"
        ));
    }

    #[test]
    fn a_scheme_or_host_change_is_never_a_match() {
        assert!(!url_matches_entry(
            "http://example.com/",
            "https://example.com"
        ));
        assert!(!url_matches_entry(
            "https://other.com/",
            "https://example.com"
        ));
    }

    #[test]
    fn an_invalid_entry_matches_nothing() {
        assert!(!url_matches_entry("https://example.com/", "https://"));
        assert!(!url_matches_entry(
            "https://example.com/",
            "http://example.com/"
        ));
    }

    #[test]
    fn any_entry_is_enough() {
        let list = vec![
            "https://a.example".to_string(),
            "https://b.example/".to_string(),
        ];
        assert!(url_is_allowlisted("https://b.example/x", &list));
        assert!(!url_is_allowlisted("https://c.example/", &list));
        assert!(!url_is_allowlisted("https://a.example/", &[]));
    }
}
