// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Parser and validator for `tpt-shell.toml` (spec §10.x, M2).
//!
//! Every error has a stable `TS####` code and, where the problem is in the text, the line
//! it is on. Sections for later phases (kiosk, devices, audit, agent, replay, test,
//! profiles, compliance) are accepted and kept as raw TOML; their own parsers come with
//! those phases. Unknown keys are rejected everywhere else.

use std::collections::HashSet;
use std::fmt;

use serde::Deserialize;
use toml::Spanned;

/// Error codes. Stable: they are published in the error registry (`tpt-shell explain`).
pub mod codes {
    /// The file is not valid TOML, or a value has the wrong type.
    pub const SYNTAX: &str = "TS0001";
    /// A key is not part of the schema.
    pub const UNKNOWN_KEY: &str = "TS0002";
    /// A value is present but not allowed (empty name, bad size, bad capability name).
    pub const INVALID_VALUE: &str = "TS0003";
    /// A required key is missing.
    pub const MISSING_KEY: &str = "TS0004";
}

/// One problem found in a manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    pub code: &'static str,
    /// 1-based line, when the problem is located in the text.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} line {line}: {}", self.code, self.message),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

impl std::error::Error for ManifestError {}

/// A validated manifest.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub app: App,
    pub windows: Vec<Window>,
    pub capabilities: Vec<String>,
    pub audit: AuditSettings,
    /// Raw sections for later phases, keyed by section name.
    pub stubs: Vec<(String, toml::Table)>,
}

/// `[audit]`: what the audit writer keeps from action parameters. The defaults keep only the
/// hash, which is what `tpt_shell_audit::Policy::default()` does too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditSettings {
    /// Keep a copy of the parameters, with `redact` applied. Off means only the hash.
    pub store_params: bool,
    /// Object keys whose values are replaced with `[redacted]` before the parameters are kept.
    pub redact: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub name: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
}

const LATER_SECTIONS: [&str; 8] = [
    "kiosk",
    "devices",
    "audit",
    "agent",
    "replay",
    "test",
    "profiles",
    "compliance",
];

const MAX_NAME_LEN: usize = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    app: Option<RawApp>,
    #[serde(default)]
    windows: Vec<RawWindow>,
    capabilities: Option<RawCapabilities>,
    kiosk: Option<toml::Table>,
    devices: Option<toml::Table>,
    audit: Option<RawAudit>,
    agent: Option<toml::Table>,
    replay: Option<toml::Table>,
    test: Option<toml::Table>,
    profiles: Option<toml::Table>,
    compliance: Option<toml::Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApp {
    name: Option<Spanned<String>>,
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    name: Option<Spanned<String>>,
    title: Option<String>,
    width: Option<Spanned<f64>>,
    height: Option<Spanned<f64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAudit {
    store_params: Option<bool>,
    #[serde(default)]
    redact: Vec<Spanned<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapabilities {
    #[serde(default)]
    grant: Vec<Spanned<String>>,
}

/// Parses and validates the text of a `tpt-shell.toml`.
pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| from_toml_error(text, &e))?;
    let line_of = |offset: usize| text[..offset.min(text.len())].matches('\n').count() + 1;

    let app = raw.app.ok_or_else(|| ManifestError {
        code: codes::MISSING_KEY,
        line: None,
        message: "missing section [app]".into(),
    })?;
    let name = app.name.ok_or_else(|| ManifestError {
        code: codes::MISSING_KEY,
        line: None,
        message: "missing key `name` in [app]".into(),
    })?;
    check_name(&name, "app.name", line_of)?;
    let app = App {
        name: name.get_ref().clone(),
        version: app.version,
    };

    let mut seen = HashSet::new();
    let mut windows = Vec::new();
    for w in raw.windows {
        let name = w.name.ok_or_else(|| ManifestError {
            code: codes::MISSING_KEY,
            line: None,
            message: "missing key `name` in [[windows]]".into(),
        })?;
        check_name(&name, "windows.name", line_of)?;
        if !seen.insert(name.get_ref().clone()) {
            return Err(invalid(
                line_of(name.span().start),
                format!("window name `{}` is used twice", name.get_ref()),
            ));
        }
        let width = positive(w.width, "width", line_of)?;
        let height = positive(w.height, "height", line_of)?;
        windows.push(Window {
            name: name.get_ref().clone(),
            title: w.title.unwrap_or_else(|| name.get_ref().clone()),
            width,
            height,
        });
    }

    let mut capabilities = Vec::new();
    for cap in raw.capabilities.map(|c| c.grant).unwrap_or_default() {
        if !is_capability(cap.get_ref()) {
            return Err(invalid(
                line_of(cap.span().start),
                format!(
                    "`{}` is not a capability name; use dotted lowercase words, e.g. `fs.read`",
                    cap.get_ref()
                ),
            ));
        }
        capabilities.push(cap.get_ref().clone());
    }

    let audit = match raw.audit {
        None => AuditSettings::default(),
        Some(audit) => {
            for key in &audit.redact {
                check_name(key, "audit.redact", line_of)?;
            }
            AuditSettings {
                store_params: audit.store_params.unwrap_or(false),
                redact: audit.redact.iter().map(|k| k.get_ref().clone()).collect(),
            }
        }
    };

    let stubs = [
        (LATER_SECTIONS[0], raw.kiosk),
        (LATER_SECTIONS[1], raw.devices),
        (LATER_SECTIONS[3], raw.agent),
        (LATER_SECTIONS[4], raw.replay),
        (LATER_SECTIONS[5], raw.test),
        (LATER_SECTIONS[6], raw.profiles),
        (LATER_SECTIONS[7], raw.compliance),
    ]
    .into_iter()
    .filter_map(|(name, table)| table.map(|t| (name.to_string(), t)))
    .collect();

    Ok(Manifest {
        app,
        windows,
        capabilities,
        audit,
        stubs,
    })
}

fn invalid(line: usize, message: String) -> ManifestError {
    ManifestError {
        code: codes::INVALID_VALUE,
        line: Some(line),
        message,
    }
}

fn check_name(
    name: &Spanned<String>,
    field: &str,
    line_of: impl Fn(usize) -> usize,
) -> Result<(), ManifestError> {
    let value = name.get_ref();
    let ok = !value.is_empty()
        && value.len() <= MAX_NAME_LEN
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(invalid(
            line_of(name.span().start),
            format!("{field} `{value}` must be 1-{MAX_NAME_LEN} ASCII letters, digits, `-` or `_`"),
        ))
    }
}

fn positive(
    value: Option<Spanned<f64>>,
    field: &str,
    line_of: impl Fn(usize) -> usize,
) -> Result<f64, ManifestError> {
    let value = value.ok_or_else(|| ManifestError {
        code: codes::MISSING_KEY,
        line: None,
        message: format!("missing key `{field}` in [[windows]]"),
    })?;
    let n = *value.get_ref();
    if n.is_finite() && n > 0.0 {
        Ok(n)
    } else {
        Err(invalid(
            line_of(value.span().start),
            format!("{field} must be a positive number, got {n}"),
        ))
    }
}

/// A capability is one or more lowercase words joined by dots, e.g. `fs.read`, `device.serial`.
fn is_capability(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                && part.starts_with(|c: char| c.is_ascii_lowercase())
        })
}

/// Maps a TOML error to a `TS####` error. Unknown keys get their own code.
fn from_toml_error(text: &str, err: &toml::de::Error) -> ManifestError {
    let message = err.message().to_string();
    let line = err
        .span()
        .map(|span| text[..span.start.min(text.len())].matches('\n').count() + 1);
    let code = if message.contains("unknown field") {
        codes::UNKNOWN_KEY
    } else if message.contains("missing field") {
        codes::MISSING_KEY
    } else {
        codes::SYNTAX
    };
    ManifestError {
        code,
        line,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
[app]
name = "demo-app"
version = "1.2.0"

[[windows]]
name = "main"
title = "Demo"
width = 800
height = 600

[capabilities]
grant = ["fs.read", "device.serial"]

[kiosk]
mode = "single-app"
"#;

    fn err(text: &str) -> ManifestError {
        parse(text).unwrap_err()
    }

    #[test]
    fn parses_a_valid_manifest() {
        let m = parse(GOOD).unwrap();
        assert_eq!(m.app.name, "demo-app");
        assert_eq!(m.app.version.as_deref(), Some("1.2.0"));
        assert_eq!(m.windows[0].title, "Demo");
        assert_eq!(m.capabilities, vec!["fs.read", "device.serial"]);
        assert_eq!(m.stubs.len(), 1);
        assert_eq!(m.stubs[0].0, "kiosk");
    }

    #[test]
    fn window_title_defaults_to_its_name() {
        let m = parse("[app]\nname = \"a\"\n[[windows]]\nname = \"w\"\nwidth = 1\nheight = 1\n")
            .unwrap();
        assert_eq!(m.windows[0].title, "w");
    }

    #[test]
    fn unknown_key_is_rejected_with_its_line() {
        let text = "[app]\nname = \"a\"\nnmae = \"typo\"\n";
        let e = err(text);
        assert_eq!(e.code, codes::UNKNOWN_KEY);
        assert_eq!(e.line, Some(3));
    }

    #[test]
    fn syntax_error_has_a_line() {
        let e = err("[app]\nname = \"a\n");
        assert_eq!(e.code, codes::SYNTAX);
        assert!(e.line.is_some());
    }

    #[test]
    fn missing_app_section_is_reported() {
        let e = err("[capabilities]\ngrant = []\n");
        assert_eq!(e.code, codes::MISSING_KEY);
        assert!(e.message.contains("[app]"));
    }

    #[test]
    fn bad_app_name_points_at_its_line() {
        let e = err("[app]\n\nname = \"has space\"\n");
        assert_eq!(e.code, codes::INVALID_VALUE);
        assert_eq!(e.line, Some(3));
    }

    #[test]
    fn overlong_app_name_is_rejected() {
        let name = "a".repeat(MAX_NAME_LEN + 1);
        let e = err(&format!("[app]\nname = \"{name}\"\n"));
        assert_eq!(e.code, codes::INVALID_VALUE);
    }

    #[test]
    fn non_positive_window_size_is_rejected() {
        let text = "[app]\nname = \"a\"\n[[windows]]\nname = \"w\"\nwidth = 0\nheight = 10\n";
        let e = err(text);
        assert_eq!(e.code, codes::INVALID_VALUE);
        assert_eq!(e.line, Some(5));
    }

    #[test]
    fn duplicate_window_names_are_rejected() {
        let text = "[app]\nname = \"a\"\n[[windows]]\nname = \"w\"\nwidth = 1\nheight = 1\n[[windows]]\nname = \"w\"\nwidth = 1\nheight = 1\n";
        let e = err(text);
        assert_eq!(e.code, codes::INVALID_VALUE);
        assert_eq!(e.line, Some(8));
    }

    #[test]
    fn capability_names_are_checked() {
        for bad in ["", "FS.read", "fs..read", "fs.1read", ".fs", "fs read"] {
            let text = format!("[app]\nname = \"a\"\n[capabilities]\ngrant = [\"{bad}\"]\n");
            assert_eq!(err(&text).code, codes::INVALID_VALUE, "accepted `{bad}`");
        }
    }

    #[test]
    fn later_phase_sections_are_accepted_as_stubs() {
        let text = "[app]\nname = \"a\"\n[profiles]\nkind = \"anything\"\n";
        let m = parse(text).unwrap();
        assert_eq!(m.stubs[0].0, "profiles");
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    const SCHEMA: &str = include_str!("../schema/tpt-shell.manifest.schema.json");

    fn schema() -> serde_json::Value {
        serde_json::from_str(SCHEMA).expect("schema is valid JSON")
    }

    #[test]
    fn schema_lists_exactly_the_sections_the_parser_accepts() {
        let schema = schema();
        let mut schema_sections: Vec<&str> = schema["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        schema_sections.sort();
        let mut parser_sections = vec!["app", "windows", "capabilities"];
        parser_sections.extend(LATER_SECTIONS);
        parser_sections.sort();
        assert_eq!(schema_sections, parser_sections);
    }

    /// Every app and window key the schema lists must parse, and a key it does not list must
    /// be refused. The parser's structs deny unknown fields, so this ties the two together.
    #[test]
    fn schema_app_and_window_keys_are_the_ones_the_parser_accepts() {
        let schema = schema();
        let app = &schema["properties"]["app"]["properties"];
        let window = &schema["properties"]["windows"]["items"]["properties"];
        let mut app_keys: Vec<&str> = app
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut window_keys: Vec<&str> = window
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        app_keys.sort();
        window_keys.sort();
        assert_eq!(app_keys, ["name", "version"]);
        assert_eq!(window_keys, ["height", "name", "title", "width"]);

        let full = "[app]
name = \"demo\"
version = \"1.0.0\"
\
            [[windows]]
name = \"main\"
title = \"Main\"
width = 640
height = 480
";
        assert!(parse(full).is_ok());
        assert!(parse(
            "[app]
name = \"demo\"
bogus = 1
"
        )
        .is_err());
        let extra_window = format!(
            "{full}[[windows]]
name = \"x\"
title = \"X\"
width = 1
height = 1
bogus = 1
"
        );
        assert!(parse(&extra_window).is_err());
    }

    #[test]
    fn schema_name_pattern_matches_the_parser_rule() {
        let pattern = schema()["properties"]["app"]["properties"]["name"]["pattern"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(pattern, "^[A-Za-z0-9_-]+$");
        assert_eq!(MAX_NAME_LEN, 64);
    }

    #[test]
    fn the_good_example_parses() {
        assert!(parse(
            "[app]\nname = \"demo\"\n[[windows]]\nname = \"main\"\nwidth = 1\nheight = 1\n"
        )
        .is_ok());
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    /// Deterministic xorshift generator, so failures reproduce.
    fn rng(seed: u64) -> impl FnMut() -> u64 {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        }
    }

    #[test]
    fn random_manifest_text_never_panics() {
        // Characters that matter to TOML and to our checks.
        let alphabet: &[u8] = b"[]=\"\n .abcxyz019-_#{}'\\t,:+";
        let mut next = rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..20_000 {
            let len = (next() % 96) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(next() as usize) % alphabet.len()] as char)
                .collect();
            let _ = parse(&text);
        }
    }

    #[test]
    fn mutated_valid_manifests_never_panic() {
        let base = "[app]\nname = \"demo\"\n[[windows]]\nname = \"main\"\nwidth = 800\nheight = 600\n[capabilities]\ngrant = [\"fs.read\"]\n";
        let mut next = rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..5_000 {
            let mut bytes = base.as_bytes().to_vec();
            for _ in 0..1 + (next() % 4) {
                let at = (next() as usize) % bytes.len();
                bytes[at] = b" \"[]=\n.x9"[(next() as usize) % 9];
            }
            if let Ok(text) = String::from_utf8(bytes) {
                let _ = parse(&text);
            }
        }
    }
}

#[cfg(test)]
mod audit_section_tests {
    use super::*;

    fn with_app(extra: &str) -> String {
        format!("[app]\nname = \"demo\"\n{extra}")
    }

    #[test]
    fn without_an_audit_section_only_hashes_are_kept() {
        let m = parse(&with_app("")).unwrap();
        assert_eq!(m.audit, AuditSettings::default());
        assert!(!m.audit.store_params);
        assert!(m.audit.redact.is_empty());
    }

    #[test]
    fn audit_section_sets_storage_and_redaction() {
        let m = parse(&with_app(
            "[audit]\nstore_params = true\nredact = [\"password\", \"api-token\"]\n",
        ))
        .unwrap();
        assert!(m.audit.store_params);
        assert_eq!(m.audit.redact, ["password", "api-token"]);
        assert!(m.stubs.is_empty(), "audit is parsed, not kept as a stub");
    }

    #[test]
    fn a_redact_name_with_a_space_is_refused_at_its_line() {
        let err = parse(&with_app("[audit]\nredact = [\"ok\", \"bad key\"]\n")).unwrap_err();
        assert_eq!(err.code, codes::INVALID_VALUE);
        assert_eq!(err.line, Some(4));
        assert!(err.message.starts_with("audit.redact `bad key`"));
    }

    #[test]
    fn schema_audit_keys_are_the_ones_the_parser_accepts() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../schema/tpt-shell.manifest.schema.json")).unwrap();
        let keys: Vec<&str> = schema["properties"]["audit"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["redact", "store_params"]);
        let full = with_app(
            "[audit]
store_params = true
redact = [\"a\"]
",
        );
        assert!(parse(&full).is_ok());
    }

    #[test]
    fn an_unknown_key_in_audit_is_refused() {
        let err = parse(&with_app("[audit]\nstore_paramz = true\n")).unwrap_err();
        assert_eq!(err.code, codes::UNKNOWN_KEY);
    }
}
