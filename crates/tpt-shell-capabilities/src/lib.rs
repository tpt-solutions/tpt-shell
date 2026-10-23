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
//! - [`Scope::PathGlobs`]: path patterns, where `*` and `?` match within one segment and
//!   `**` matches any number of segments.
//! - [`Scope::Devices`]: device identifiers, or globs over them, each naming a device kind.
//! - [`Scope::Urls`]: URL prefixes, matched on a path-segment boundary.
//! - [`Scope::MaxBytes`]: a size cap.
//!
//! A grant with `consent: true` asks the user once per window and capability. The answer
//! is kept in a [`ConsentStore`]. The default store is in memory. [`FileConsent`] keeps the
//! answers in a file, which the app places in the profile folder.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// What a grant lets through.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    Unrestricted,
    /// Directory roots such as `C:/Data`. A request must be the root or below it.
    Paths(Vec<String>),
    /// Path patterns such as `C:/Data/**/*.txt`. A request must match one pattern in full.
    PathGlobs(Vec<String>),
    /// Device identifiers such as `serial:COM3`, or globs such as `serial:COM*` or
    /// `usb:0416:*`. `*` matches any run of characters and `?` one character. Matching ignores
    /// ASCII case. Each entry must name a device kind (`serial`, `hid`, `usb` or `net`), and a
    /// `device.<kind>` grant may only name that kind (see [`Engine::add_grant`]).
    Devices(Vec<String>),
    /// URL prefixes such as `https://example.com/app`.
    Urls(Vec<String>),
    /// Requests of at most this many bytes.
    MaxBytes(u64),
}

/// One permission given to one window and origin.
///
/// Support: Full.
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
///
/// Support: Full.
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
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Denial {
    /// Stable code for the page: `CAP_NO_GRANT`, `CAP_OUT_OF_SCOPE`, `CAP_BAD_REQUEST`,
    /// or `CAP_CONSENT_DECLINED`.
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Support: Full.
pub enum Decision {
    Allow,
    Deny(Denial),
    /// A grant needs consent and none is stored. Call [`Engine::authorize`] to ask.
    NeedsConsent,
}

/// Rejected when a grant is added with a scope that cannot be used safely.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantError(pub String);

/// Asks the user. Implemented by the platform (a native dialog, never web content).
///
/// Support: Full.
pub trait ConsentPrompt {
    fn ask(&mut self, window: &str, capability: &str) -> bool;
}

/// Remembers consent answers, keyed by window and capability.
///
/// Support: Full.
pub trait ConsentStore {
    fn get(&self, window: &str, capability: &str) -> Option<bool>;
    fn set(&mut self, window: &str, capability: &str, allowed: bool);
}

/// In-memory consent store.
///
/// Support: Full.
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

/// Consent answers kept in a file, so the user is asked once per profile rather than once per
/// run. The file is one line per answer: `allow` or `deny`, a tab, the window, a tab, and the
/// capability. Each save writes a temporary file and renames it over the old one.
///
/// A save that fails keeps the answer for this run only, so the user may be asked again next
/// start. Lines that do not parse are skipped, which asks again rather than allowing.
///
/// Support: Full.
pub struct FileConsent {
    path: PathBuf,
    answers: HashMap<(String, String), bool>,
}

impl FileConsent {
    /// Opens the store at `path`. A missing file is an empty store.
    ///
    /// Support: Full.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let answers = text
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                let allowed = match parts.next()? {
                    "allow" => true,
                    "deny" => false,
                    _ => return None,
                };
                let window = parts.next()?;
                let capability = parts.next()?;
                Some(((window.to_string(), capability.to_string()), allowed))
            })
            .collect();
        Ok(FileConsent { path, answers })
    }

    fn save(&self) -> io::Result<()> {
        let mut text = String::new();
        for ((window, capability), allowed) in &self.answers {
            let word = if *allowed { "allow" } else { "deny" };
            text.push_str(&format!("{word}\t{window}\t{capability}\n"));
        }
        let mut tmp = self.path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }
}

impl ConsentStore for FileConsent {
    fn get(&self, window: &str, capability: &str) -> Option<bool> {
        self.answers
            .get(&(window.to_string(), capability.to_string()))
            .copied()
    }

    fn set(&mut self, window: &str, capability: &str, allowed: bool) {
        // Tabs and newlines would break the line format; such a name is kept for this run only.
        let storable = |s: &str| !s.contains(['\t', '\n', '\r']);
        self.answers
            .insert((window.to_string(), capability.to_string()), allowed);
        if storable(window) && storable(capability) {
            let _ = self.save();
        }
    }
}

/// Holds grants and checks requests against them.
///
/// Support: Full.
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
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses `store` for consent answers instead of the in-memory default.
    ///
    /// Support: Full.
    pub fn with_consent_store(store: Box<dyn ConsentStore>) -> Self {
        Engine {
            grants: Vec::new(),
            consent: store,
        }
    }

    /// Adds a grant. Path roots are normalized now, so a bad root fails here, not per request.
    ///
    /// Support: Full.
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
        if let Scope::PathGlobs(patterns) = &grant.scope {
            for pattern in patterns {
                normalize_path(pattern)
                    .map_err(|why| GrantError(format!("path pattern `{pattern}`: {why}")))?;
            }
        }
        if let Scope::Devices(ids) = &grant.scope {
            for id in ids {
                check_device_entry(&grant.capability, id).map_err(GrantError)?;
            }
        }
        self.grants.push(grant);
        Ok(())
    }

    /// Checks a request without asking anyone.
    ///
    /// Support: Full.
    pub fn check(
        &self,
        window: &str,
        origin: &str,
        capability: &str,
        request: Request,
    ) -> Decision {
        self.check_with(window, origin, capability, request, false)
    }

    /// The body of [`Engine::check`]. With `resolve_roots`, granted roots are compared in their
    /// real form too, so a real path (from [`real_path`]) matches a root given through a link
    /// or a short (8.3) name.
    fn check_with(
        &self,
        window: &str,
        origin: &str,
        capability: &str,
        request: Request,
        resolve_roots: bool,
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
            match in_scope(&grant.scope, request, resolve_roots) {
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
    ///
    /// Support: Full.
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

impl Engine {
    /// Like [`Engine::check`] for a path, but also checks where the path really points. The
    /// path is first checked lexically; then its real location (symbolic links and junctions
    /// followed, see [`real_path`]) must also be inside the scope. A path that cannot be
    /// resolved is denied.
    ///
    /// Support: Partial. The on-disk check follows symbolic links and junctions, which is tested on
    /// Windows only; not run on macOS/Linux here; compile-checked.
    pub fn check_path_on_disk(
        &self,
        window: &str,
        origin: &str,
        capability: &str,
        raw: &str,
    ) -> Decision {
        match self.check(window, origin, capability, Request::Path(raw)) {
            Decision::Allow => self.check_real(window, origin, capability, raw),
            other => other,
        }
    }

    /// Like [`Engine::check_path_on_disk`], but asks `prompt` when consent is needed.
    ///
    /// Support: Partial. The on-disk check follows symbolic links and junctions, which is tested on
    /// Windows only; not run on macOS/Linux here; compile-checked.
    pub fn authorize_path_on_disk(
        &mut self,
        prompt: &mut dyn ConsentPrompt,
        window: &str,
        origin: &str,
        capability: &str,
        raw: &str,
    ) -> Decision {
        match self.authorize(prompt, window, origin, capability, Request::Path(raw)) {
            Decision::Allow => self.check_real(window, origin, capability, raw),
            other => other,
        }
    }

    fn check_real(&self, window: &str, origin: &str, capability: &str, raw: &str) -> Decision {
        match real_path(raw) {
            Ok(real) => self.check_with(window, origin, capability, Request::Path(&real), true),
            Err(why) => Decision::Deny(bad_request(format!(
                "`{raw}` cannot be checked against the scope: {why}"
            ))),
        }
    }
}

/// The real location of `raw` on disk, normalized like [`normalize_path`]. Symbolic links and
/// junctions are followed, so a link inside a granted root that points outside it gives a path
/// outside the root. A path that does not exist yet (a file to be created) is resolved through
/// its parent directory. A dangling link is an error, never a path to trust.
///
/// Support: Partial. Link following is tested on Windows only; not run on macOS/Linux here;
/// compile-checked.
pub fn real_path(raw: &str) -> Result<String, String> {
    let path = std::path::Path::new(raw);
    let real = match fs::canonicalize(path) {
        Ok(real) => real,
        Err(err)
            if err.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(path).is_err() =>
        {
            let parent = path.parent().ok_or("the path has no parent directory")?;
            let name = path.file_name().ok_or("the path has no file name")?;
            fs::canonicalize(parent)
                .map_err(|e| format!("the parent directory cannot be resolved: {e}"))?
                .join(name)
        }
        Err(err) => return Err(format!("the path cannot be resolved: {err}")),
    };
    let text = real.to_string_lossy().into_owned();
    // Windows returns verbatim paths (`\\?\C:\...`). The prefix is not part of the path.
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    normalize_path(&text)
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

fn in_scope(scope: &Scope, request: Request, resolve_roots: bool) -> Result<(), Denial> {
    match scope {
        Scope::Unrestricted => Ok(()),
        Scope::Paths(roots) => {
            let Request::Path(raw) = request else {
                return Err(bad_request("this grant needs a path request"));
            };
            let path = normalize_path(raw).map_err(bad_request)?;
            let inside = roots.iter().any(|root| {
                // A root that cannot be resolved (no such directory) keeps its lexical form.
                let root = match resolve_roots.then(|| real_path(root)) {
                    Some(Ok(real)) => real,
                    _ => normalize_path(root).unwrap_or_default(),
                };
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
        Scope::PathGlobs(patterns) => {
            let Request::Path(raw) = request else {
                return Err(bad_request("this grant needs a path request"));
            };
            let path = normalize_path(raw).map_err(bad_request)?;
            let inside = patterns.iter().any(|pattern| {
                normalize_path(pattern).is_ok_and(|pattern| glob_matches(&pattern, &path))
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
            // A request names one device. A wildcard in it would let a grant for `serial:*`
            // match the text `serial:*` itself, so it is refused outright.
            if id.contains(['*', '?']) {
                return Err(bad_request("a device identifier cannot contain `*` or `?`"));
            }
            if ids.iter().any(|allowed| device_glob_matches(allowed, id)) {
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
///
/// Support: Full.
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
/// Matches a normalized path against a normalized pattern, segment by segment.
fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    segments_match(&pattern, &path)
}

fn segments_match(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            segments_match(rest, path)
                || path
                    .split_first()
                    .is_some_and(|(_, tail)| segments_match(pattern, tail))
        }
        Some((segment, rest)) => path.split_first().is_some_and(|(first, tail)| {
            wildcard_matches(segment.as_bytes(), first.as_bytes()) && segments_match(rest, tail)
        }),
    }
}

/// `*` matches any run of characters and `?` one character, within a single segment.
fn wildcard_matches(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.split_first(), text.split_first()) {
        (None, None) => true,
        (Some((b'*', rest)), _) => {
            wildcard_matches(rest, text)
                || text
                    .split_first()
                    .is_some_and(|(_, tail)| wildcard_matches(pattern, tail))
        }
        (Some((b'?', rest)), Some((_, tail))) => wildcard_matches(rest, tail),
        (Some((p, rest)), Some((t, tail))) => p == t && wildcard_matches(rest, tail),
        _ => false,
    }
}

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

/// The device kinds an identifier can start with. These match the prefixes of
/// `tpt_shell_devices::id::DeviceId`, which this crate does not depend on.
const DEVICE_KINDS: [&str; 4] = ["serial", "hid", "usb", "net"];

/// The one kind a `device.<kind>` capability may name. `device.print` and other capabilities name
/// no single kind, so they return `None`.
fn device_kind_of(capability: &str) -> Option<&'static str> {
    DEVICE_KINDS
        .iter()
        .copied()
        .find(|kind| capability.strip_prefix("device.") == Some(*kind))
}

/// Checks one `Scope::Devices` entry when its grant is added. The entry must start with a known
/// kind, and a `device.<kind>` grant must name that kind, so `device.serial` cannot reach an HID
/// device even through a glob.
fn check_device_entry(capability: &str, entry: &str) -> Result<(), String> {
    let kind = entry
        .split_once(':')
        .map(|(kind, _)| kind)
        .unwrap_or_default();
    if !DEVICE_KINDS.contains(&kind) {
        return Err(format!(
            "device `{entry}` must start with a kind: serial, hid, usb or net"
        ));
    }
    if let Some(expected) = device_kind_of(capability) {
        if kind != expected {
            return Err(format!(
                "`{capability}` grants may only name {expected} devices, not `{entry}`"
            ));
        }
    }
    Ok(())
}

/// Whether the device pattern `pattern` covers the identifier `id`. A pattern with no wildcards
/// is an exact match. Case is ignored, as it is for port and host names in the devices crate.
fn device_glob_matches(pattern: &str, id: &str) -> bool {
    let (pattern, id) = (pattern.as_bytes(), id.as_bytes());
    let (mut p, mut t) = (0, 0);
    // The last `*` seen, and the text position it has consumed up to. Backtracking goes only
    // there, so the match is linear in the pattern times the text, with no exponential case.
    let mut star: Option<(usize, usize)> = None;
    while t < id.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len()
            && (pattern[p] == b'?' || pattern[p].eq_ignore_ascii_case(&id[t]))
        {
            p += 1;
            t += 1;
        } else if let Some((star_p, star_t)) = star {
            p = star_p + 1;
            t = star_t + 1;
            star = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
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
    fn path_globs_match_a_whole_path() {
        let e = engine_with(Scope::PathGlobs(vec!["C:/Data/*.txt".into()]));
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::Path("C:/Data/a.txt")),
            Decision::Allow
        );
        for bad in [
            "C:/Data/sub/a.txt",
            "C:/Data/a.bin",
            "C:/Data",
            "C:/Data/a.txt.bak",
        ] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_OUT_OF_SCOPE",
                "{bad}"
            );
        }
    }

    #[test]
    fn double_star_matches_any_depth_and_question_mark_one_character() {
        let e = engine_with(Scope::PathGlobs(vec![
            "C:/Data/**".into(),
            "C:/Logs/log?.csv".into(),
        ]));
        for ok in [
            "C:/Data",
            "C:/Data/a",
            "C:/Data/x/y/z.bin",
            "C:/Logs/log1.csv",
        ] {
            assert_eq!(
                e.check("main", ORIGIN, "fs.read", Request::Path(ok)),
                Decision::Allow,
                "{ok}"
            );
        }
        for bad in ["C:/Logs/log12.csv", "C:/Database/a", "D:/Data/a"] {
            assert_eq!(
                code(e.check("main", ORIGIN, "fs.read", Request::Path(bad))),
                "CAP_OUT_OF_SCOPE",
                "{bad}"
            );
        }
    }

    #[test]
    fn globs_do_not_widen_the_path_rules() {
        let e = engine_with(Scope::PathGlobs(vec!["C:/Data/**".into()]));
        for bad in [
            "C:/Data/../Secret/a",
            "\\\\server\\share\\Data\\a",
            "C:/Data/CON",
        ] {
            assert_ne!(
                e.check("main", ORIGIN, "fs.read", Request::Path(bad)),
                Decision::Allow,
                "{bad}"
            );
        }
    }

    #[test]
    fn bad_path_patterns_fail_when_the_grant_is_added() {
        let mut e = Engine::new();
        for bad in ["C:/Data/../x/*", "relative/*", "C:/Da\0ta/*", "\\\\?\\C:/*"] {
            let result = e.add_grant(grant(
                "main",
                ORIGIN,
                "fs.read",
                Scope::PathGlobs(vec![bad.into()]),
            ));
            assert!(result.is_err(), "{bad}");
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

    fn device_engine(capability: &str, ids: &[&str]) -> Engine {
        let mut e = Engine::new();
        let scope = Scope::Devices(ids.iter().map(|id| id.to_string()).collect());
        e.add_grant(grant("main", ORIGIN, capability, scope))
            .unwrap();
        e
    }

    #[test]
    fn a_serial_grant_cannot_open_an_hid_device() {
        let e = device_engine("device.serial", &["serial:*"]);
        assert_eq!(
            e.check(
                "main",
                ORIGIN,
                "device.serial",
                Request::Device("serial:COM3")
            ),
            Decision::Allow
        );
        assert_eq!(
            code(e.check(
                "main",
                ORIGIN,
                "device.serial",
                Request::Device("hid:1234:5678")
            )),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn a_net_grant_cannot_open_a_serial_port() {
        let e = device_engine("device.net", &["net:*:9100"]);
        assert_eq!(
            e.check(
                "main",
                ORIGIN,
                "device.net",
                Request::Device("net:printer.local:9100")
            ),
            Decision::Allow
        );
        assert_eq!(
            code(e.check("main", ORIGIN, "device.net", Request::Device("serial:COM3"))),
            "CAP_OUT_OF_SCOPE"
        );
        assert_eq!(
            code(e.check(
                "main",
                ORIGIN,
                "device.net",
                Request::Device("net:printer.local:9101")
            )),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn a_device_grant_that_names_another_kind_is_refused_when_added() {
        let mut e = Engine::new();
        for (capability, entry) in [
            ("device.serial", "hid:*"),
            ("device.hid", "serial:COM3"),
            ("device.usb", "net:*:9100"),
            ("device.net", "*"),
            ("device.print", "bluetooth:AA"),
            ("device.print", "*:*"),
        ] {
            let scope = Scope::Devices(vec![entry.into()]);
            assert!(
                e.add_grant(grant("main", ORIGIN, capability, scope))
                    .is_err(),
                "{capability} accepted {entry}"
            );
        }
        let scope = Scope::Devices(vec!["usb:0416:5011".into()]);
        assert!(e
            .add_grant(grant("main", ORIGIN, "device.print", scope))
            .is_ok());
    }

    #[test]
    fn device_globs_ignore_case_and_stay_inside_their_kind() {
        let e = device_engine("device.print", &["usb:0416:*", "serial:com*"]);
        for allowed in ["usb:0416:5011", "serial:COM3"] {
            assert_eq!(
                e.check("main", ORIGIN, "device.print", Request::Device(allowed)),
                Decision::Allow,
                "{allowed} should be inside the scope"
            );
        }
        for refused in ["usb:0417:5011", "net:printer.local:9100"] {
            assert_eq!(
                code(e.check("main", ORIGIN, "device.print", Request::Device(refused))),
                "CAP_OUT_OF_SCOPE",
                "{refused} should be outside the scope"
            );
        }
    }

    #[test]
    fn a_request_that_names_a_wildcard_is_refused_not_matched() {
        let e = device_engine("device.serial", &["serial:*"]);
        assert_eq!(
            code(e.check("main", ORIGIN, "device.serial", Request::Device("serial:*"))),
            "CAP_BAD_REQUEST"
        );
    }

    #[test]
    fn the_glob_matcher_handles_stars_and_question_marks() {
        assert!(device_glob_matches("serial:COM*", "serial:COM12"));
        assert!(device_glob_matches("serial:COM?", "serial:com3"));
        assert!(device_glob_matches("usb:*:5011", "usb:0416:5011"));
        assert!(device_glob_matches("serial:COM3", "SERIAL:com3"));
        assert!(!device_glob_matches("serial:COM?", "serial:COM12"));
        assert!(!device_glob_matches("serial:COM3", "serial:COM30"));
        assert!(!device_glob_matches("usb:*:5011", "usb:0416:5012"));
        assert!(device_glob_matches("*", ""));
        assert!(!device_glob_matches("?", ""));
    }

    #[test]
    fn consent_for_an_hid_grant_is_asked_once_and_kept_in_the_profile() {
        let path = scratch_file("consent-hid.txt");
        let _ = fs::remove_file(&path);
        let mut g = grant(
            "main",
            ORIGIN,
            "device.hid",
            Scope::Devices(vec!["hid:1234:*".into()]),
        );
        g.consent = true;
        {
            let mut e = Engine::with_consent_store(Box::new(FileConsent::open(&path).unwrap()));
            e.add_grant(g.clone()).unwrap();
            let mut prompt = Answer(true, 0);
            assert_eq!(
                e.authorize(
                    &mut prompt,
                    "main",
                    ORIGIN,
                    "device.hid",
                    Request::Device("hid:1234:5678")
                ),
                Decision::Allow
            );
            assert_eq!(prompt.1, 1);
        }
        let mut e = Engine::with_consent_store(Box::new(FileConsent::open(&path).unwrap()));
        e.add_grant(g).unwrap();
        let mut prompt = Answer(false, 0);
        assert_eq!(
            e.authorize(
                &mut prompt,
                "main",
                ORIGIN,
                "device.hid",
                Request::Device("hid:1234:5678")
            ),
            Decision::Allow
        );
        assert_eq!(prompt.1, 0, "the answer should be remembered across runs");
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

    fn scratch_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-consent-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn file_consent_answers_survive_a_restart() {
        let path = scratch_file("consent-restart.txt");
        let _ = fs::remove_file(&path);
        {
            let mut store = FileConsent::open(&path).unwrap();
            store.set("main", "device.serial", true);
            store.set("other", "device.serial", false);
        }
        let store = FileConsent::open(&path).unwrap();
        assert_eq!(store.get("main", "device.serial"), Some(true));
        assert_eq!(store.get("other", "device.serial"), Some(false));
        assert_eq!(store.get("main", "fs.read"), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn file_consent_skips_lines_it_cannot_read() {
        let path = scratch_file("consent-bad-lines.txt");
        fs::write(
            &path,
            "allow\tmain\tdevice.serial\nmaybe\tmain\tfs.read\nallow\tmain\n",
        )
        .unwrap();
        let store = FileConsent::open(&path).unwrap();
        assert_eq!(store.get("main", "device.serial"), Some(true));
        assert_eq!(store.get("main", "fs.read"), None);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn a_remembered_answer_is_not_asked_again_after_restart() {
        let path = scratch_file("consent-engine.txt");
        let _ = fs::remove_file(&path);
        {
            let mut e = Engine::with_consent_store(Box::new(FileConsent::open(&path).unwrap()));
            let mut g = grant("main", ORIGIN, "device.serial", Scope::Unrestricted);
            g.consent = true;
            e.add_grant(g).unwrap();
            let mut prompt = Answer(true, 0);
            e.authorize(&mut prompt, "main", ORIGIN, "device.serial", Request::None);
            assert_eq!(prompt.1, 1);
        }
        let mut e = Engine::with_consent_store(Box::new(FileConsent::open(&path).unwrap()));
        let mut g = grant("main", ORIGIN, "device.serial", Scope::Unrestricted);
        g.consent = true;
        e.add_grant(g).unwrap();
        assert_eq!(
            e.check("main", ORIGIN, "device.serial", Request::None),
            Decision::Allow
        );
        let _ = fs::remove_file(&path);
    }

    /// A fresh directory under the system temp dir, unique per test run.
    fn fresh_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir =
            std::env::temp_dir().join(format!("tpt-scope-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Makes a directory link, or returns `false` when this machine does not allow links
    /// (Windows needs developer mode or elevation). The caller then skips the link checks.
    fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(target, link).or_else(|_| {
            // A junction needs no privilege, unlike a symbolic link, so it is the fallback
            // when developer mode is off. It is what `mklink /J` makes.
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .map_err(io::Error::other)
                .and_then(|out| {
                    if out.status.success() {
                        Ok(())
                    } else {
                        Err(io::Error::other("mklink /J failed"))
                    }
                })
        });
        #[cfg(not(any(unix, windows)))]
        let made: io::Result<()> = Err(io::Error::other("no links here"));
        if let Err(why) = &made {
            eprintln!("skipping link checks: cannot create a directory link ({why})");
        }
        made.is_ok()
    }

    fn engine_for_root(root: &std::path::Path) -> Engine {
        let mut e = Engine::new();
        e.add_grant(grant(
            "main",
            ORIGIN,
            "fs.read",
            Scope::Paths(vec![root.to_string_lossy().into_owned()]),
        ))
        .unwrap();
        e
    }

    #[test]
    fn a_link_inside_the_root_that_points_outside_is_denied() {
        let root = fresh_dir("root");
        let outside = fresh_dir("outside");
        fs::write(outside.join("secret.txt"), b"x").unwrap();
        if !link_dir(&outside, &root.join("escape")) {
            return;
        }
        let e = engine_for_root(&root);
        let via_link = root.join("escape").join("secret.txt");
        let raw = via_link.to_string_lossy().into_owned();
        // Lexically the path is inside the root, so only the real-path check refuses it.
        assert_eq!(
            e.check("main", ORIGIN, "fs.read", Request::Path(&raw)),
            Decision::Allow
        );
        assert_eq!(
            code(e.check_path_on_disk("main", ORIGIN, "fs.read", &raw)),
            "CAP_OUT_OF_SCOPE"
        );
    }

    #[test]
    fn a_file_inside_the_root_is_allowed_on_disk() {
        let root = fresh_dir("inside");
        fs::write(root.join("a.txt"), b"x").unwrap();
        let e = engine_for_root(&root);
        let raw = root.join("a.txt").to_string_lossy().into_owned();
        assert_eq!(
            e.check_path_on_disk("main", ORIGIN, "fs.read", &raw),
            Decision::Allow
        );
        // A file that does not exist yet is resolved through its parent.
        let new_file = root.join("new.txt").to_string_lossy().into_owned();
        assert_eq!(
            e.check_path_on_disk("main", ORIGIN, "fs.read", &new_file),
            Decision::Allow
        );
    }

    /// The grant and the request both use the link path (as a short 8.3 name would be spelled),
    /// while the file really lives under the target. The real path must match the real root.
    #[test]
    fn a_file_under_a_root_given_through_a_link_is_allowed_on_disk() {
        let target = fresh_dir("target");
        fs::write(target.join("a.txt"), b"x").unwrap();
        let alias = fresh_dir("alias").join("root");
        if !link_dir(&target, &alias) {
            return;
        }
        let e = engine_for_root(&alias);
        let raw = alias.join("a.txt").to_string_lossy().into_owned();
        assert_eq!(
            e.check_path_on_disk("main", ORIGIN, "fs.read", &raw),
            Decision::Allow
        );
    }

    #[test]
    fn a_dangling_link_is_denied_not_trusted() {
        let root = fresh_dir("dangling");
        let gone = fresh_dir("gone");
        if !link_dir(&gone, &root.join("dangling")) {
            return;
        }
        // The link now points at nothing, but it is still there.
        fs::remove_dir_all(&gone).unwrap();
        let e = engine_for_root(&root);
        let raw = root.join("dangling").to_string_lossy().into_owned();
        assert!(matches!(
            e.check_path_on_disk("main", ORIGIN, "fs.read", &raw),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn real_paths_are_normalized_and_lowercased() {
        let root = fresh_dir("case");
        fs::write(root.join("MiXed.TXT"), b"x").unwrap();
        let real = real_path(&root.join("MiXed.TXT").to_string_lossy()).unwrap();
        assert_eq!(real, real.to_ascii_lowercase());
        assert!(!real.contains('\\'), "{real}");
        assert!(!real.starts_with("//"), "{real}");
    }

    #[test]
    fn a_path_that_is_not_absolute_is_refused_on_disk() {
        let e = Engine::new();
        assert!(matches!(
            e.check_path_on_disk("main", ORIGIN, "fs.read", "relative/a.txt"),
            Decision::Deny(_)
        ));
    }
}
