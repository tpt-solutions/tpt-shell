// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Compliance outputs (spec §10.8, M10): SBOMs in two formats, licence notices, an attestation,
//! detached signatures, and a licence gate. The input is the JSON from
//! `cargo metadata --format-version 1 --locked`, the lock file, and `deny.toml`.
//!
//! Output is deterministic. Components are sorted by name and version, and there is no
//! timestamp unless the caller passes one.
//!
//! The licence gate uses the same rule as cargo-deny's allow list: an expression passes when
//! one of its OR branches is entirely on the allow list, and an AND branch needs every licence
//! on it. The allow list is read from `deny.toml` at run time ([`allow_list_from_deny_toml`]).
//! The gate is this crate's own parser, not cargo-deny itself; CI still runs cargo-deny.

pub mod lock;
pub mod notices;
pub mod schema;

use serde_json::{json, Value};

/// `deny.toml` could not be read, or has no licence allow list. Fix: check the file has a
/// `[licenses] allow = [...]` array.
pub const CODE_POLICY: &str = "TS0120";
/// A generated SBOM does not match its published schema. Fix: the message names the path.
pub const CODE_SCHEMA: &str = "TS0121";

/// Licences the gate accepts when no `deny.toml` is given. Keep in step with `deny.toml`; the
/// test `allow_list_matches_deny_toml` checks this.
pub const ALLOWED_LICENSES: [&str; 4] = ["MIT", "Unicode-3.0", "BSD-3-Clause", "ISC"];

/// A third-party crate from the lock file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Component {
    pub name: String,
    pub version: String,
    /// SPDX licence expression as written in the crate's manifest, if any.
    pub license: Option<String>,
    pub repository: Option<String>,
}

/// The application being described.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub version: String,
}

/// Finds the application named `name` in `cargo metadata` output and returns its name and version.
pub fn app_from_metadata(json_text: &str, name: &str) -> Result<App, String> {
    let doc: Value =
        serde_json::from_str(json_text).map_err(|e| format!("metadata is not JSON: {e}"))?;
    let package = doc
        .get("packages")
        .and_then(Value::as_array)
        .and_then(|packages| {
            packages
                .iter()
                .find(|p| p.get("name").and_then(Value::as_str) == Some(name))
        })
        .ok_or_else(|| format!("no package named `{name}` in the metadata"))?;
    let version = package
        .get("version")
        .and_then(Value::as_str)
        .ok_or("the package has no version")?;
    Ok(App {
        name: name.to_string(),
        version: version.to_string(),
    })
}

/// Reads the third-party packages from `cargo metadata` output. Workspace members (packages
/// without a `source`) are left out; the application itself is described by [`App`].
pub fn components_from_metadata(json_text: &str) -> Result<Vec<Component>, String> {
    let doc: Value =
        serde_json::from_str(json_text).map_err(|e| format!("metadata is not JSON: {e}"))?;
    let packages = doc
        .get("packages")
        .and_then(Value::as_array)
        .ok_or("metadata has no `packages` array")?;
    let mut out = Vec::new();
    for package in packages {
        if package.get("source").is_none_or(Value::is_null) {
            continue;
        }
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .ok_or("a package has no name")?;
        let version = package
            .get("version")
            .and_then(Value::as_str)
            .ok_or("a package has no version")?;
        out.push(Component {
            name: name.to_string(),
            version: version.to_string(),
            license: package
                .get("license")
                .and_then(Value::as_str)
                .map(str::to_string),
            repository: package
                .get("repository")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// CycloneDX 1.5 JSON.
pub fn cyclonedx(components: &[Component], app: &App, timestamp: Option<&str>) -> Value {
    let mut metadata = json!({
        "component": { "type": "application", "name": app.name, "version": app.version },
        "supplier": { "name": "TPT Solutions" },
    });
    if let Some(ts) = timestamp {
        metadata["timestamp"] = json!(ts);
    }
    let items: Vec<Value> = components
        .iter()
        .map(|c| {
            let mut item = json!({
                "type": "library",
                "bom-ref": purl(c),
                "name": c.name,
                "version": c.version,
                "purl": purl(c),
            });
            if let Some(license) = &c.license {
                item["licenses"] = json!([{ "expression": license }]);
            }
            if let Some(repo) = &c.repository {
                item["externalReferences"] = json!([{ "type": "vcs", "url": repo }]);
            }
            item
        })
        .collect();
    json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": metadata,
        "components": items,
    })
}

/// SPDX 2.3 JSON. `namespace` must be a unique URI for this document.
pub fn spdx(components: &[Component], app: &App, namespace: &str, created: Option<&str>) -> Value {
    let mut packages = vec![json!({
        "SPDXID": "SPDXRef-Application",
        "name": app.name,
        "versionInfo": app.version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": false,
        "supplier": "Organization: TPT Solutions",
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": "MIT OR Apache-2.0",
        "copyrightText": "NOASSERTION",
    })];
    for (i, c) in components.iter().enumerate() {
        let license = c.license.clone().unwrap_or_else(|| "NOASSERTION".into());
        let mut item = json!({
            "SPDXID": format!("SPDXRef-Package-{}", i + 1),
            "name": c.name,
            "versionInfo": c.version,
            "downloadLocation": c.repository.clone().unwrap_or_else(|| "NOASSERTION".into()),
            "filesAnalyzed": false,
            "licenseConcluded": license,
            "licenseDeclared": license,
            "copyrightText": "NOASSERTION",
            "externalRefs": [{
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": purl(c),
            }],
        });
        if c.license.is_none() {
            item["licenseDeclared"] = json!("NOASSERTION");
        }
        packages.push(item);
    }
    let mut creation = json!({ "creators": ["Tool: tpt-shell"] });
    if let Some(ts) = created {
        creation["created"] = json!(ts);
    }
    json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": format!("{} {}", app.name, app.version),
        "documentNamespace": namespace,
        "creationInfo": creation,
        "documentDescribes": ["SPDXRef-Application"],
        "packages": packages,
    })
}

fn purl(c: &Component) -> String {
    format!("pkg:cargo/{}@{}", c.name, c.version)
}

/// The allow list of the built-in policy, as owned strings.
pub fn default_allow() -> Vec<String> {
    ALLOWED_LICENSES.iter().map(|s| s.to_string()).collect()
}

/// Reads the licence allow list from the text of a `deny.toml`. Fails when the file is not
/// TOML or has no `[licenses] allow` array of strings.
pub fn allow_list_from_deny_toml(deny_toml: &str) -> Result<Vec<String>, String> {
    let table: toml::Table = deny_toml
        .parse()
        .map_err(|e| format!("{CODE_POLICY}: deny.toml is not valid TOML: {e}"))?;
    let allow = table
        .get("licenses")
        .and_then(|l| l.get("allow"))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{CODE_POLICY}: deny.toml has no [licenses] allow list"))?;
    allow
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{CODE_POLICY}: a licence in deny.toml is not a string"))
        })
        .collect()
}

/// Whether a licence expression is acceptable under the built-in allow list.
///
/// The grammar is SPDX-like: identifiers, `AND` (binds tighter), `OR`, parentheses, and
/// `WITH` (an exception, which does not change the licence it is attached to). The older
/// `/` form is read as `OR`. An expression with anything else in it is refused.
pub fn license_allowed(expression: &str) -> bool {
    license_allowed_with(expression, &default_allow())
}

/// Like [`license_allowed`], against the given allow list (for example from `deny.toml`).
pub fn license_allowed_with(expression: &str, allow: &[String]) -> bool {
    match tokenize(expression) {
        Some(tokens) if !tokens.is_empty() => {
            let mut parser = Parser {
                tokens,
                pos: 0,
                allow,
            };
            match parser.or_expr() {
                Some(allowed) if parser.pos == parser.tokens.len() => allowed,
                _ => false,
            }
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    And,
    Or,
    With,
    Open,
    Close,
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '/' => {
                chars.next();
                tokens.push(Token::Or);
            }
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+') {
                        word.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(match word.as_str() {
                    "AND" => Token::And,
                    "OR" => Token::Or,
                    "WITH" => Token::With,
                    _ => Token::Ident(word),
                });
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// Recursive-descent evaluation of the expression against the allow list. Each function
/// returns whether that part is allowed, or `None` when the tokens do not parse.
struct Parser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    allow: &'a [String],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn or_expr(&mut self) -> Option<bool> {
        let mut any = self.and_expr()?;
        while self.peek() == Some(&Token::Or) {
            self.pos += 1;
            any |= self.and_expr()?;
        }
        Some(any)
    }

    fn and_expr(&mut self) -> Option<bool> {
        let mut all = self.with_expr()?;
        while self.peek() == Some(&Token::And) {
            self.pos += 1;
            all &= self.with_expr()?;
        }
        Some(all)
    }

    fn with_expr(&mut self) -> Option<bool> {
        let atom = self.atom()?;
        if self.peek() == Some(&Token::With) {
            self.pos += 1;
            match self.peek() {
                Some(Token::Ident(_)) => self.pos += 1,
                _ => return None,
            }
        }
        Some(atom)
    }

    fn atom(&mut self) -> Option<bool> {
        match self.peek()?.clone() {
            Token::Open => {
                self.pos += 1;
                let inner = self.or_expr()?;
                if self.peek() != Some(&Token::Close) {
                    return None;
                }
                self.pos += 1;
                Some(inner)
            }
            Token::Ident(name) => {
                self.pos += 1;
                Some(self.allow.contains(&name))
            }
            _ => None,
        }
    }
}

/// Components whose licence the gate refuses, as `name version: reason`. An empty result
/// means the gate passes.
pub fn license_violations(components: &[Component]) -> Vec<String> {
    license_violations_with(components, &default_allow())
}

/// Like [`license_violations`], against the given allow list.
pub fn license_violations_with(components: &[Component], allow: &[String]) -> Vec<String> {
    components
        .iter()
        .filter_map(|c| match &c.license {
            None => Some(format!("{} {}: no licence declared", c.name, c.version)),
            Some(expr) if !license_allowed_with(expr, allow) => Some(format!(
                "{} {}: licence `{expr}` is not on the allow list",
                c.name, c.version
            )),
            Some(_) => None,
        })
        .collect()
}

/// How the build was made reproducible. Recorded in the attestation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reproducibility {
    /// The `SOURCE_DATE_EPOCH` used for timestamps, if one was set.
    pub source_date_epoch: Option<u64>,
    /// Whether the build used the committed lock file (`--locked`).
    pub locked: bool,
    /// Extra flags the build was run with, such as path remapping.
    pub cargo_flags: Vec<String>,
}

/// The attestation: what was built, from which lock file, and what the gate found. The policy
/// section lists the built-in allow list. Use [`attestation_with_allow`] for `deny.toml`'s list.
pub fn attestation(
    app: &App,
    rustc: &str,
    lockfile_sha256: &str,
    outputs: &[(&str, String)],
    violations: &[String],
    git_commit: Option<&str>,
    reproducibility: &Reproducibility,
) -> Value {
    attestation_with_allow(
        app,
        rustc,
        lockfile_sha256,
        outputs,
        violations,
        git_commit,
        reproducibility,
        &default_allow(),
    )
}

/// [`attestation`], recording the allow list that the gate actually used.
#[allow(clippy::too_many_arguments)]
pub fn attestation_with_allow(
    app: &App,
    rustc: &str,
    lockfile_sha256: &str,
    outputs: &[(&str, String)],
    violations: &[String],
    git_commit: Option<&str>,
    reproducibility: &Reproducibility,
    allow: &[String],
) -> Value {
    let files: serde_json::Map<String, Value> = outputs
        .iter()
        .map(|(name, digest)| ((*name).to_string(), json!({ "sha256": digest })))
        .collect();
    json!({
        "app": { "name": app.name, "version": app.version },
        "rustc": rustc,
        "git_commit": git_commit,
        "lockfile_sha256": lockfile_sha256,
        "outputs": files,
        "reproducibility": {
            "source_date_epoch": reproducibility.source_date_epoch,
            "locked": reproducibility.locked,
            "cargo_flags": reproducibility.cargo_flags,
        },
        "policy": {
            "name": "licence allow list",
            "allowed": allow,
            "result": if violations.is_empty() { "pass" } else { "fail" },
            "violations": violations,
        },
    })
}

/// SHA-256 of `bytes`, as used for the attestation's digests.
pub fn digest(bytes: &[u8]) -> String {
    tpt_shell_audit::sha256_hex(bytes)
}

/// The licence gate for a build: every component must pass the allow list, and every crate the
/// lock file pins must be described by the metadata. Returns the violations; empty means pass.
pub fn gate(
    components: &[Component],
    locked: &[lock::LockedPackage],
    allow: &[String],
) -> Vec<String> {
    let mut violations = license_violations_with(components, allow);
    violations.extend(lock::unknown_to_metadata(locked, components));
    violations
}

/// The message for a failed gate, with its code. `violations` must not be empty.
pub fn gate_failure(violations: &[String]) -> String {
    format!(
        "{CODE_GATE}: the licence gate refused {} item(s): {}",
        violations.len(),
        violations.join("; ")
    )
}

/// `TS0122`: the licence gate refused a component. Fix: replace the crate, or add its licence to
/// `deny.toml` with a justification line.
pub const CODE_GATE: &str = "TS0122";

/// Signs an artifact with the audit key (Ed25519 over the file's SHA-256). Returns the signature
/// as 128 hex digits, to be written next to the artifact, for example `sbom.cdx.json.sig`.
pub fn sign_artifact(
    signer: &dyn tpt_shell_audit::keys::CheckpointSigner,
    bytes: &[u8],
) -> Result<String, String> {
    tpt_shell_audit::keys::sign_detached(signer, bytes).map_err(|e| e.to_string())
}

/// Checks a signature made by [`sign_artifact`]. `public_key_hex` is the public key as 64 hex
/// digits, as the audit tools write it.
pub fn verify_artifact(
    public_key_hex: &str,
    bytes: &[u8],
    signature_hex: &str,
) -> Result<(), String> {
    let key = tpt_shell_audit::verifying_key_from_hex(public_key_hex)
        .ok_or("the public key is not 64 hex digits")?;
    tpt_shell_audit::keys::verify_detached(&key, bytes, signature_hex).map_err(|e| e.to_string())
}

/// Formats Unix seconds as an ISO 8601 UTC timestamp, for example `2023-11-14T22:13:20Z`.
pub fn iso8601_utc(epoch_seconds: u64) -> String {
    let days = (epoch_seconds / 86_400) as i64;
    let secs = epoch_seconds % 86_400;
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const METADATA: &str = r#"{
        "packages": [
            {"name":"tpt-shell","version":"0.0.0","source":null,"license":"MIT OR Apache-2.0","repository":null},
            {"name":"serde_json","version":"1.0.1","source":"registry+x","license":"MIT OR Apache-2.0","repository":"https://github.com/serde-rs/json"},
            {"name":"zlib-thing","version":"2.0.0","source":"registry+x","license":"Zlib","repository":null},
            {"name":"mystery","version":"0.1.0","source":"registry+x","license":null,"repository":null},
            {"name":"aaa","version":"0.1.0","source":"registry+x","license":"MIT","repository":null}
        ]
    }"#;

    fn app() -> App {
        App {
            name: "demo".into(),
            version: "1.0.0".into(),
        }
    }

    #[test]
    fn workspace_members_are_left_out() {
        let components = components_from_metadata(METADATA).unwrap();
        assert!(components.iter().all(|c| c.name != "tpt-shell"));
        assert_eq!(components.len(), 4);
    }

    #[test]
    fn components_are_sorted_by_name() {
        let names: Vec<String> = components_from_metadata(METADATA)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, vec!["aaa", "mystery", "serde_json", "zlib-thing"]);
    }

    #[test]
    fn cyclonedx_has_the_required_top_level_fields() {
        let components = components_from_metadata(METADATA).unwrap();
        let bom = cyclonedx(&components, &app(), None);
        assert_eq!(bom["bomFormat"], "CycloneDX");
        assert_eq!(bom["specVersion"], "1.5");
        assert_eq!(bom["metadata"]["component"]["name"], "demo");
        let serde = bom["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "serde_json")
            .unwrap();
        assert_eq!(serde["purl"], "pkg:cargo/serde_json@1.0.1");
        assert_eq!(serde["licenses"][0]["expression"], "MIT OR Apache-2.0");
        assert!(bom["metadata"].get("timestamp").is_none());
    }

    #[test]
    fn spdx_lists_the_application_and_every_package() {
        let components = components_from_metadata(METADATA).unwrap();
        let doc = spdx(&components, &app(), "https://example.invalid/demo-1", None);
        assert_eq!(doc["spdxVersion"], "SPDX-2.3");
        assert_eq!(doc["dataLicense"], "CC0-1.0");
        let packages = doc["packages"].as_array().unwrap();
        assert_eq!(packages.len(), 5, "application plus four components");
        let mystery = packages.iter().find(|p| p["name"] == "mystery").unwrap();
        assert_eq!(mystery["licenseDeclared"], "NOASSERTION");
    }

    #[test]
    fn the_gate_accepts_or_branches_with_an_allowed_licence() {
        assert!(license_allowed("MIT OR Apache-2.0"));
        assert!(license_allowed("Apache-2.0 OR MIT"));
        assert!(license_allowed("MIT"));
        assert!(license_allowed("ISC"));
        assert!(license_allowed("Unicode-3.0 AND MIT"));
        assert!(license_allowed("MIT/Apache-2.0"), "the slash form is OR");
        assert!(license_allowed("(MIT OR Apache-2.0) AND Unicode-3.0"));
        assert!(license_allowed("MIT WITH LLVM-exception OR Apache-2.0"));
    }

    #[test]
    fn the_gate_refuses_and_branches_with_a_forbidden_licence() {
        assert!(!license_allowed("MIT AND GPL-2.0"));
        assert!(!license_allowed("Apache-2.0"));
        assert!(!license_allowed("Zlib"));
        assert!(!license_allowed("GPL-3.0 WITH Classpath-exception-2.0"));
        assert!(license_allowed("(MIT OR Apache-2.0) AND MIT"));
        assert!(!license_allowed("(Apache-2.0 OR GPL-2.0) AND MIT"));
        assert!(!license_allowed("MIT (OR"));
        assert!(!license_allowed("MIT; GPL"));
        assert!(!license_allowed(""));
    }

    #[test]
    fn the_gate_follows_the_allow_list_it_is_given() {
        let only_zlib = vec!["Zlib".to_string()];
        assert!(license_allowed_with("Zlib", &only_zlib));
        assert!(!license_allowed_with("MIT", &only_zlib));
        assert!(!license_allowed_with("MIT", &[]));
    }

    #[test]
    fn the_allow_list_is_read_from_deny_toml() {
        let list = allow_list_from_deny_toml(
            "[licenses]\nallow = [\"MIT\", \"ISC\"]\n[bans]\nmultiple-versions = \"warn\"\n",
        )
        .unwrap();
        assert_eq!(list, vec!["MIT", "ISC"]);
        let err = allow_list_from_deny_toml("[bans]\n").unwrap_err();
        assert!(err.starts_with(CODE_POLICY));
        assert!(allow_list_from_deny_toml("[licenses]\nallow = [1]\n").is_err());
        assert!(allow_list_from_deny_toml("not = [toml").is_err());
    }

    #[test]
    fn violations_name_each_failing_component() {
        let components = components_from_metadata(METADATA).unwrap();
        let violations = license_violations(&components);
        assert_eq!(violations.len(), 2);
        assert!(violations
            .iter()
            .any(|v| v.starts_with("mystery 0.1.0: no licence")));
        assert!(violations
            .iter()
            .any(|v| v.contains("zlib-thing") && v.contains("Zlib")));
    }

    #[test]
    fn attestation_records_the_gate_result() {
        let att = attestation(
            &app(),
            "rustc 1.97.1",
            "abc",
            &[("sbom.cdx.json", digest(b"x"))],
            &[],
            Some("deadbeef"),
            &Reproducibility {
                source_date_epoch: Some(1_700_000_000),
                locked: true,
                cargo_flags: vec!["--locked".into()],
            },
        );
        assert_eq!(att["policy"]["result"], "pass");
        assert_eq!(att["git_commit"], "deadbeef");
        assert_eq!(att["reproducibility"]["source_date_epoch"], 1_700_000_000);
        assert_eq!(att["reproducibility"]["locked"], true);
        let repro = Reproducibility {
            source_date_epoch: Some(1),
            locked: true,
            cargo_flags: vec![],
        };
        let failing = attestation(&app(), "rustc", "abc", &[], &["bad".into()], None, &repro);
        assert_eq!(failing["policy"]["result"], "fail");
        assert!(failing["git_commit"].is_null());
    }

    #[test]
    fn outputs_are_deterministic() {
        let components = components_from_metadata(METADATA).unwrap();
        assert_eq!(
            cyclonedx(&components, &app(), None).to_string(),
            cyclonedx(&components, &app(), None).to_string()
        );
    }

    #[test]
    fn allow_list_matches_deny_toml() {
        let deny = include_str!("../../../deny.toml");
        let mut from_file = allow_list_from_deny_toml(deny).unwrap();
        from_file.sort();
        let mut built_in = default_allow();
        built_in.sort();
        assert_eq!(
            from_file, built_in,
            "ALLOWED_LICENSES must be kept in step with deny.toml"
        );
    }
}

#[cfg(test)]
mod time_tests {
    use super::iso8601_utc;

    #[test]
    fn known_timestamps_format_correctly() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z", "leap day");
        assert_eq!(iso8601_utc(1_709_251_199), "2024-02-29T23:59:59Z");
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn public_hex(key: &SigningKey) -> String {
        key.verifying_key()
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn signed_artifacts_verify_and_catch_edits() {
        let key = SigningKey::from_bytes(&[11; 32]);
        let sbom = b"{\"bomFormat\":\"CycloneDX\"}";
        let sig = sign_artifact(&key, sbom).unwrap();
        assert!(verify_artifact(&public_hex(&key), sbom, &sig).is_ok());
        assert!(verify_artifact(&public_hex(&key), b"{}", &sig).is_err());
        let other = SigningKey::from_bytes(&[12; 32]);
        assert!(verify_artifact(&public_hex(&other), sbom, &sig).is_err());
        assert!(verify_artifact("short", sbom, &sig).is_err());
    }

    #[test]
    fn the_gate_combines_licence_and_lock_checks() {
        let components = vec![
            Component {
                name: "ok".into(),
                version: "1.0.0".into(),
                license: Some("MIT".into()),
                repository: None,
            },
            Component {
                name: "copyleft".into(),
                version: "2.0.0".into(),
                license: Some("GPL-3.0-only".into()),
                repository: None,
            },
        ];
        let locked = vec![("ok".to_string(), "1.0.0".to_string())];
        let violations = gate(&components, &locked, &default_allow());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("GPL-3.0-only"));
        let failure = gate_failure(&violations);
        assert!(failure.starts_with(CODE_GATE));

        let unlisted = vec![("ghost".to_string(), "0.1.0".to_string())];
        let violations = gate(&components[..1], &unlisted, &default_allow());
        assert_eq!(violations.len(), 1, "a locked crate with no metadata fails");
        assert!(gate(&components[..1], &[], &default_allow()).is_empty());
    }
}
