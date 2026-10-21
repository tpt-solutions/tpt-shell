// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Permission engine (spec §9, §10.x; M2).
//!
//! Default deny: a request is allowed only when a grant names the capability, the window
//! and the origin, and the request is inside the grant's scope. Grants are per window and
//! per origin, so one window cannot use another window's grants.
//!
//! Scopes:
//! - [`Scope::Paths`]: directory roots. Paths are normalized first (see [`normalize_path`]).
//! - [`Scope::Devices`]: exact device identifiers.
//! - [`Scope::Urls`]: URL prefixes, matched on a path-segment boundary.
//! - [`Scope::MaxBytes`]: a size cap.
//!
//! A grant with `consent: true` asks the user once per window and capability. The answer
//! is kept in a [`ConsentStore`]. The default store is in memory; the profile store comes
//! with M6.

use std::collections::HashMap;

/// What a grant lets through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Unrestricted,
    /// Directory roots such as `C:/Data`. A request must be the root or below it.
    Paths(Vec<String>),
    /// Exact device identifiers such as `serial:COM3`.
    Devices(Vec<String>),
    /// URL prefixes such as `https://example.com/app`.
    Urls(Vec<String>),
    /// Requests of at most this many bytes.
    MaxBytes(u64),
}

/// One permission given to one window and origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub capability: String,
    pub window: String,
    pub origin: String,
    pub scope: Scope,
    /// Ask the user the first time, then remember the answer.
    pub consent: bool,
}

/// What a command wants to do, checked against the scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request<'a> {
    /// No target: only unrestricted grants allow it.
    None,
    Path(&'a str),
    Device(&'a str),
    Url(&'a str),
    Bytes(u64),
}

/// Why a request was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denial {
    /// Stable code for the page: `CAP_NO_GRANT`, `CAP_OUT_OF_SCOPE`, `CAP_BAD_REQUEST`,
    /// or `CAP_CONSENT_DECLINED`.
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(Denial),
    /// A grant needs consent and none is stored. Call [`Engine::authorize`] to ask.
    NeedsConsent,
}

/// Rejected when a grant is added with a scope that cannot be used safely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantError(pub String);

/// Asks the user. Implemented by the platform (a native dialog, never web content).
pub trait ConsentPrompt {
    fn ask(&mut self, window: &str, capability: &str) -> bool;
}

/// Remembers consent answers, keyed by window and capability.
pub trait ConsentStore {
    fn get(&self, window: &str, capability: &str) -> Option<bool>;
    fn set(&mut self, window: &str, capability: &str, allowed: bool);
}

/// In-memory consent store.
#[derive(Default)]
pub struct MemoryConsent {
    answers: HashMap<(String, String), bool>,
}

impl ConsentStore for MemoryConsent {
    fn get(&self, window: &str, capability: &str) -> Option<bool> {
        self.answers
            .get(&(window.to_string(), capability.to_string()))
            .copied()
    }

    fn set(&mut self, window: &str, capability: &str, allowed: bool) {
        self.answers
            .insert((window.to_string(), capability.to_string()), allowed);
    }
}

/// Holds grants and checks requests against them.
pub struct Engine {
    grants: Vec<Grant>,
    consent: Box<dyn ConsentStore>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            grants: Vec::new(),
            consent: Box::new(MemoryConsent::default()),
        }
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses `store` for consent answers instead of the in-memory default.
    pub fn with_consent_store(store: Box<dyn ConsentStore>) -> Self {
        Engine {
            grants: Vec::new(),
            consent: store,
        }
    }

    /// Adds a grant. Path roots are normalized now, so a bad root fails here, not per request.
    pub fn add_grant(&mut self, grant: Grant) -> Result<(), GrantError> {
        if !is_capability_name(&grant.capability) {
            return Err(GrantError(format!(
                "`{}` is not a capability name",
                grant.capability
            )));
        }
        if let Scope::Paths(roots) = &grant.scope {
            for root in roots {
                normalize_path(root)
                    .map_err(|why| GrantError(format!("path root `{root}`: {why}")))?;
            }
        }
        self.grants.push(grant);
        Ok(())
    }

    /// Checks a request without asking anyone.
    pub fn check(
        &self,
        window: &str,
        origin: &str,
        capability: &str,
        request: Request,
    ) -> Decision {
        let mut matching = self
            .grants
            .iter()
            .filter(|g| g.window == window && g.origin == origin && g.capability == capability)
            .peekable();
        if matching.peek().is_none() {
            return Decision::Deny(Denial {
                code: "CAP_NO_GRANT",
                message: format!("`{capability}` is not granted to window `{window}`"),
            });
        }

        let mut out_of_scope = None;
        for grant in matching {
            match in_scope(&grant.scope, request) {
                Ok(()) => {
                    return if grant.consent {
                        match self.consent.get(window, capability) {
                            Some(true) => Decision::Allow,
                            Some(false) => Decision::Deny(declined(capability)),
                            None => Decision::NeedsConsent,
                        }
                    } else {
                        Decision::Allow
                    };
                }
                Err(denial) => out_of_scope = Some(denial),
            }
        }
        Decision::Deny(out_of_scope.unwrap_or(Denial {
            code: "CAP_OUT_OF_SCOPE",
            message: format!("request is outside the scope granted for `{capability}`"),
        }))
    }

    /// Like [`Engine::check`], but asks `prompt` when consent is needed, and stores the answer.
    pub fn authorize(
        &mut self,
        prompt: &mut dyn ConsentPrompt,
        window: &str,
        origin: &str,
        capability: &str,
        request: Request,
    ) -> Decision {
        match self.check(window, origin, capability, request) {
            Decision::NeedsConsent => {
                let allowed = prompt.ask(window, capability);
                self.consent.set(window, capability, allowed);
                if allowed {
                    Decision::Allow
                } else {
                    Decision::Deny(declined(capability))
                }
            }
            other => other,
        }
    }
}

fn declined(capability: &str) -> Denial {
    Denial {
        code: "CAP_CONSENT_DECLINED",
        message: format!("the user declined `{capability}`"),
    }
}

fn bad_request(message: impl Into<String>) -> Denial {
    Denial {
        code: "CAP_BAD_REQUEST",
        message: message.into(),
    }
}

fn outside(capability_hint: &str) -> Denial {
    Denial {
        code: "CAP_OUT_OF_SCOPE",
        message: format!("request is outside the scope granted for {capability_hint}"),
    }
}

fn in_scope(scope: &Scope, request: Request) -> Result<(), Denial> {
    match scope {
        Scope::Unrestricted => Ok(()),
        Scope::Paths(roots) => {
            let Request::Path(raw) = request else {
                return Err(bad_request("this grant needs a path request"));
            };
            let path = normalize_path(raw).map_err(bad_request)?;
            let inside = roots.iter().any(|root| {
                let root = normalize_path(root).unwrap_or_default();
                path == root
                    || path.starts_with(&root)
                        && (root.ends_with('/') || path[root.len()..].starts_with('/'))
            });
            if inside {
                Ok(())
            } else {
                Err(outside("the path"))
            }
        }
        Scope::Devices(ids) => {
            let Request::Device(id) = request else {
                return Err(bad_request("this grant needs a device request"));
            };
            if ids.iter().any(|allowed| allowed == id) {
                Ok(())
            } else {
                Err(outside("the device"))
            }
        }
        Scope::Urls(prefixes) => {
            let Request::Url(raw) = request else {
                return Err(bad_request("this grant needs a URL request"));
            };
            let url = normalize_url(raw).map_err(bad_request)?;
            let inside = prefixes.iter().any(|prefix| {
                url.starts_with(prefix.as_str())
                    && (prefix.ends_with('/')
                        || matches!(
                            url[prefix.len()..].chars().next(),
                            None | Some('/' | '?' | '#')
                        ))
            });
            if inside {
                Ok(())
            } else {
                Err(outside("the URL"))
            }
        }
        Scope::MaxBytes(limit) => {
            let Request::Bytes(size) = request else {
                return Err(bad_request("this grant needs a size"));
            };
            if size <= *limit {
                Ok(())
            } else {
                Err(outside(&format!("a {size}-byte request (limit {limit})")))
            }
        }
    }
}

/// Windows device names. A path component that starts with one of these refers to a device.
const RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Normalizes a path for scope checks and rejects anything that could escape a root.
///
/// Accepted: an absolute path with a drive letter (`C:\Data\a.txt`) or a rooted path
/// (`/data`). Rejected: relative paths, `..`, UNC and device paths (`\\server`, `\\?\`),
/// NUL bytes, alternate data streams (a colon after the drive), reserved device names
/// (`CON`, `COM1`), and components ending in `.` or space, which Windows silently strips.
///
/// The result is lowercase with `/` separators. It is for comparison only, not for opening
/// files.
pub fn normalize_path(raw: &str) -> Result<String, String> {
    if raw.contains('\0') {
        return Err("path contains a NUL byte".into());
    }
    if raw.starts_with("\\\\") || raw.starts_with("//") {
        return Err("UNC and device paths are not allowed".into());
    }
    let s = raw.replace('\\', "/");
    let bytes = s.as_bytes();
    let (drive, rest) = if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        (s[..2].to_ascii_lowercase(), &s[2..])
    } else {
        (String::new(), &s[..])
    };
    if !rest.starts_with('/') {
        return Err("only absolute paths are allowed".into());
    }
    if rest.contains(':') {
        return Err(
            "a colon is only allowed after the drive letter (alternate data streams)".into(),
        );
    }
    let mut parts = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err("parent directory `..` is not allowed".into()),
            _ => {}
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(format!(
                "`{part}` ends with a dot or space, which Windows strips"
            ));
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        if RESERVED_NAMES.contains(&stem.as_str()) {
            return Err(format!("`{part}` is a reserved device name"));
        }
        parts.push(part.to_ascii_lowercase());
    }
    let mut out = format!("{drive}/{}", parts.join("/"));
    if parts.is_empty() {
        out = format!("{drive}/");
    }
    Ok(out)
}

/// Normalizes an absolute `http(s)` URL for prefix matching. Rejects userinfo (`@`),
/// backslashes, and dot segments, including percent-encoded ones.
fn normalize_url(raw: &str) -> Result<String, String> {
    let lower = raw.to_ascii_lowercase();
    let scheme_ok = lower.starts_with("https://") || lower.starts_with("http://");
    if !scheme_ok {
        return Err("only http and https URLs are allowed".into());
    }
    if raw.contains('\\') || raw.contains('\0') {
        return Err("URL contains a backslash or NUL byte".into());
    }
    let after_scheme = raw.find("://").map(|i| i + 3).unwrap_or(0);
    let authority_end = raw[after_scheme..]
        .find(['/', '?', '#'])
        .map_or(raw.len(), |i| after_scheme + i);
    if raw[after_scheme..authority_end].contains('@') {
        return Err("URL user information (`@`) is not allowed".into());
    }
    let path = &lower[authority_end..];
    let path_only = path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .replace("%2e", ".");
    if path_only.split('/').any(|seg| seg == "..") {
        return Err("`..` segments are not allowed in URLs".into());
    }
    Ok(raw.to_string())
}

fn is_capability_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            part.starts_with(|c: char| c.is_ascii_lowercase())
                && part.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(window: &str, origin: &str, cap: &str, scope: Scope) -> Grant {
        Grant {
            capability: cap.into(),
            window: window.into(),
            origin: origin.into(),
            scope,
            consent: false,
        }
    }

    const ORIGIN: &str = "https://tpt-app.localhost";

    fn engine_with(scope: Scope) -> Engine {
        let mut e = Engine::new();
        e.add_grant(grant("main", ORIGIN, "fs.read", scope))
            .unwrap();
        e
    }

    fn code(d: Decision) -> &'static str {
        match d {
            Decision::Deny(denial) => denial.code,
            other => panic!("expected a denial, got {other:?}"),
        }
    }

    #[test]
    fn no_grant_means_denied() {
        let e = Engine::new();
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.read", Request::None)),
            "CAP_NO_GRANT"
        );
    }

    #[test]
    fn grants_are_per_window() {
        let e = engine_with(Scope::Unrestricted);
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::None),
            Decision::Allow
        );
        assert_eq!(
            code(e.check("other", ORIGIN, "fs.read", Request::None)),
            "CAP_NO_GRANT"
        );
    }

    #[test]
    fn grants_are_per_origin() {
        let e = engine_with(Scope::Unrestricted);
        assert_eq!(
            code(e.check("main", "https://evil.example", "fs.read", Request::None)),
            "CAP_NO_GRANT"
        );
    }

    #[test]
    fn grants_are_per_capability() {
        let e = engine_with(Scope::Unrestricted);
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.write", Request::None)),
            "CAP_NO_GRANT"
        );
    }

    #[test]
    fn paths_inside_a_root_are_allowed() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        for ok in [
            "C:/Data",
            "C:/Data/a.txt",
            "c:\\data\\sub\\b.txt",
            "C:/Data/",
        ] {
            assert_eq!(
                e.check("main", ORIGIN, "fs.read", Request::Path(ok)),
                Decision::Allow,
                "{ok}"
            );
        }
    }

    #[test]
    fn sibling_prefixes_are_not_inside_the_root() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.read", Request::Path("C:/Database/x"))),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn parent_directory_escapes_are_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        for bad in [
            "C:/Data/../Secret/x",
            "C:/Data\\..\\Secret",
            "C:/Data/./../x",
        ] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_BAD_REQUEST",
                "{bad}"
            );
        }
    }

    #[test]
    fn unc_and_device_paths_are_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        for bad in [
            "\\\\server\\share\\x",
            "//server/share/x",
            "\\\\?\\C:\\Data\\x",
            "\\\\.\\pipe\\x",
        ] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_BAD_REQUEST",
                "{bad}"
            );
        }
    }

    #[test]
    fn alternate_data_streams_are_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        assert_eq!(
            code(e.check(
                "main",
                ORIGIN,
                "fs.read",
                Request::Path("C:/Data/a.txt:hidden")
            )),
            "CAP_BAD_REQUEST"
        );
    }

    #[test]
    fn reserved_device_names_and_trailing_dots_are_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        for bad in [
            "C:/Data/CON",
            "C:/Data/nul.txt",
            "C:/Data/com1.log",
            "C:/Data/file. ",
            "C:/Data/x.",
        ] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_BAD_REQUEST",
                "{bad}"
            );
        }
    }

    #[test]
    fn relative_and_nul_paths_are_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        for bad in ["Data/x", "x", "C:/Data/a\0b"] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_BAD_REQUEST",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn case_differences_do_not_escape_the_root() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        assert_eq!(
            e.check(
                "main",
                ORIGIN,
                "fs.read",
                Request::Path("c:/DATA/Report.TXT")
            ),
            Decision::Allow
        );
    }

    #[test]
    fn invalid_roots_fail_when_the_grant_is_added() {
        let mut e = Engine::new();
        let err = e
            .add_grant(grant(
                "main",
                ORIGIN,
                "fs.read",
                Scope::Paths(vec!["..".into()]),
            ))
            .unwrap_err();
        assert!(err.0.contains("path root"));
    }

    #[test]
    fn device_ids_must_match_exactly() {
        let e = engine_with(Scope::Devices(vec!["serial:COM3".into()]));
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::Device("serial:COM3")),
            Decision::Allow
        );
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.read", Request::Device("serial:COM30"))),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn url_prefixes_match_on_segment_boundaries() {
        let e = engine_with(Scope::Urls(vec!["https://example.com/app".into()]));
        assert_eq!(
            e.check(
                "main",
                ORIGIN,
                "fs.read",
                Request::Url("https://example.com/app/x?y=1")
            ),
            Decision::Allow
        );
        assert_eq!(
            e.check(
                "main",
                ORIGIN,
                "fs.read",
                Request::Url("https://example.com/app")
            ),
            Decision::Allow
        );
        assert_eq!(
            code(e.check(
                "main",
                ORIGIN,
                "fs.read",
                Request::Url("https://example.com/apple")
            )),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn url_tricks_are_rejected() {
        let e = engine_with(Scope::Urls(vec!["https://example.com/".into()]));
        for bad in [
            "https://example.com@evil.example/",
            "https://example.com/../secret",
            "https://example.com/%2e%2e/secret",
            "https://example.com\\..\\secret",
            "file:///C:/secret.txt",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Url(bad))),
                "CAP_BAD_REQUEST",
                "{bad}"
            );
        }
    }

    #[test]
    fn size_caps_are_enforced() {
        let e = engine_with(Scope::MaxBytes(1024));
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::Bytes(1024)),
            Decision::Allow
        );
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.read", Request::Bytes(1025))),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn a_request_of_the_wrong_kind_is_rejected() {
        let e = engine_with(Scope::Paths(vec!["C:/Data".into()]));
        assert_eq!(
            code(e.check("main", ORIGIN, "fs.read", Request::Bytes(1))),
            "CAP_BAD_REQUEST"
        );
    }

    #[test]
    fn any_matching_grant_can_allow() {
        let mut e = Engine::new();
        e.add_grant(grant(
            "main",
            ORIGIN,
            "fs.read",
            Scope::Paths(vec!["C:/A".into()]),
        ))
        .unwrap();
        e.add_grant(grant(
            "main",
            ORIGIN,
            "fs.read",
            Scope::Paths(vec!["C:/B".into()]),
        ))
        .unwrap();
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::Path("C:/B/x")),
            Decision::Allow
        );
    }

    struct Answer(bool, usize);

    impl ConsentPrompt for Answer {
        fn ask(&mut self, _: &str, _: &str) -> bool {
            self.1 += 1;
            self.0
        }
    }

    fn consent_engine() -> Engine {
        let mut e = Engine::new();
        let mut g = grant(
            "main",
            ORIGIN,
            "device.serial",
            Scope::Devices(vec!["serial:COM3".into()]),
        );
        g.consent = true;
        e.add_grant(g).unwrap();
        e
    }

    #[test]
    fn consent_is_asked_once_and_remembered() {
        let mut e = consent_engine();
        let req = Request::Device("serial:COM3");
        assert_eq!(
            e.check("main", ORIGIN, "device.serial", req),
            Decision::NeedsConsent
        );
        let mut prompt = Answer(true, 0);
        assert_eq!(
            e.authorize(&mut prompt, "main", ORIGIN, "device.serial", req),
            Decision::Allow
        );
        assert_eq!(
            e.check("main", ORIGIN, "device.serial", req),
            Decision::Allow
        );
        assert_eq!(
            e.authorize(&mut prompt, "main", ORIGIN, "device.serial", req),
            Decision::Allow
        );
        assert_eq!(prompt.1, 1, "the user should only be asked once");
    }

    #[test]
    fn declined_consent_is_remembered_too() {
        let mut e = consent_engine();
        let req = Request::Device("serial:COM3");
        let mut prompt = Answer(false, 0);
        assert_eq!(
            code(e.authorize(&mut prompt, "main", ORIGIN, "device.serial", req)),
            "CAP_CONSENT_DECLINED"
        );
        assert_eq!(
            code(e.check("main", ORIGIN, "device.serial", req)),
            "CAP_CONSENT_DECLINED"
        );
        assert_eq!(prompt.1, 1);
    }

    #[test]
    fn consent_is_not_asked_for_out_of_scope_requests() {
        let mut e = consent_engine();
        let mut prompt = Answer(true, 0);
        assert_eq!(
            code(e.authorize(
                &mut prompt,
                "main",
                ORIGIN,
                "device.serial",
                Request::Device("serial:COM9")
            )),
            "CAP_OUT_OF_SCOPE"
        );
        assert_eq!(prompt.1, 0);
    }
}
