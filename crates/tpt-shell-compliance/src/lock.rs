// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Reads `Cargo.lock` for the gate. Every crate the lock file pins from a registry or git must
//! appear in the metadata with a licence the gate can judge. A locked crate with no metadata
//! entry would otherwise pass silently, so it is reported as a violation.

use std::collections::BTreeSet;

use crate::Component;

/// One package pinned by the lock file: name and version.
///
/// Support: Full.
pub type LockedPackage = (String, String);

/// The packages in a `Cargo.lock` that come from a source (a registry or git). Workspace
/// members have no `source` and are left out.
///
/// Support: Full.
pub fn sourced_packages(lock_text: &str) -> Result<Vec<LockedPackage>, String> {
    let table: toml::Table = lock_text
        .parse()
        .map_err(|e| format!("Cargo.lock is not valid TOML: {e}"))?;
    let Some(packages) = table.get("package").and_then(toml::Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = BTreeSet::new();
    for package in packages {
        let Some(source) = package.get("source").and_then(toml::Value::as_str) else {
            continue;
        };
        if source.is_empty() {
            continue;
        }
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or("a locked package has no name")?;
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .ok_or("a locked package has no version")?;
        out.insert((name.to_string(), version.to_string()));
    }
    Ok(out.into_iter().collect())
}

/// Locked packages that have no entry in the metadata components. Each is reported as
/// `name version: ...`, so the gate cannot pass a crate it has never seen.
///
/// Support: Full.
pub fn unknown_to_metadata(locked: &[LockedPackage], components: &[Component]) -> Vec<String> {
    locked
        .iter()
        .filter(|(name, version)| {
            !components
                .iter()
                .any(|c| c.name == *name && c.version == *version)
        })
        .map(|(name, version)| {
            format!("{name} {version}: locked but not described by cargo metadata")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"version = 4

[[package]]
name = "tpt-shell"
version = "0.0.0"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "unicode-ident"
version = "1.0.12"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    #[test]
    fn only_sourced_packages_are_listed_once() {
        let packages = sourced_packages(LOCK).unwrap();
        assert_eq!(
            packages,
            vec![
                ("serde".to_string(), "1.0.200".to_string()),
                ("unicode-ident".to_string(), "1.0.12".to_string()),
            ]
        );
    }

    #[test]
    fn a_locked_crate_missing_from_the_metadata_is_reported() {
        let locked = sourced_packages(LOCK).unwrap();
        let components = vec![Component {
            name: "serde".into(),
            version: "1.0.200".into(),
            license: Some("MIT OR Apache-2.0".into()),
            repository: None,
        }];
        let missing = unknown_to_metadata(&locked, &components);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].starts_with("unicode-ident 1.0.12"));
    }

    #[test]
    fn a_broken_lock_file_is_refused() {
        assert!(sourced_packages("[[package]\n").is_err());
        assert_eq!(sourced_packages("version = 4\n").unwrap(), vec![]);
    }
}
