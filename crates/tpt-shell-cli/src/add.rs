// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell add <feature>` (spec §10.1 to §10.4): appends a feature's section to
//! `tpt-shell.toml`. The new text goes through the manifest parser before anything is written,
//! so a file that would not check is left as it was.
//!
//! Scope: manifest sections only. The sections are the smallest ones that say what the feature
//! is for. The feature crates they configure are separate work; see todo.md.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tpt_shell_errors::lookup;

/// The features `add` writes sections for.
pub const FEATURES: [&str; 4] = ["kiosk", "devices", "audit", "agent"];

/// The manifest section for a feature. `None` for a name `add` does not know.
fn section(feature: &str) -> Option<&'static str> {
    Some(match feature {
        "kiosk" => {
            "[kiosk]\n# Single-app full-screen mode (spec \u{a7}10.1).\nmode = \"single-app\"\n"
        }
        "devices" => {
            "[devices]\n# Use virtual serial, HID and printer devices in dev and tests (spec \u{a7}10.2).\nvirtual = true\n"
        }
        "audit" => {
            "[audit]\n# Keep only the hash of action parameters unless store_params is set (spec \u{a7}10.3).\nstore_params = false\nredact = [\"password\", \"token\", \"secret\"]\n"
        }
        "agent" => {
            "[agent]\n# Agent surface is off until enabled (spec \u{a7}10.4).\nenabled = false\nmax_session_seconds = 900\n"
        }
        _ => return None,
    })
}

/// Whether the text already has a table header for `feature`, such as `[audit]` or
/// `[audit.redact]`.
fn has_section(text: &str, feature: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line == format!("[{feature}]") || line.starts_with(&format!("[{feature}."))
    })
}

/// Adds the section for `feature` to the manifest at `path`, after checking the result.
pub fn add(feature: &str, path: &Path) -> Result<ExitCode, String> {
    let Some(section) = section(feature) else {
        return Err(format!(
            "unknown feature `{feature}`; choose one of: {}",
            FEATURES.join(", ")
        ));
    };
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if has_section(&text, feature) {
        return Err(format!(
            "{} already has a [{feature}] section; edit it there",
            path.display()
        ));
    }
    let mut updated = text;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push('\n');
    updated.push_str(section);

    match tpt_shell_manifest::parse(&updated) {
        Ok(_) => {
            fs::write(path, &updated)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            println!("added [{feature}] to {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            println!(
                "{}: not changed; the new section does not check: {e}",
                path.display()
            );
            if let Some(entry) = lookup(e.code) {
                println!("  {}", entry.fix);
            }
            Ok(ExitCode::from(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "[app]\nname = \"my-app\"\nversion = \"0.1.0\"\n\n[[windows]]\nname = \"main\"\ntitle = \"My App\"\nwidth = 960\nheight = 640\n\n[capabilities]\ngrant = []\n";

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-add-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        dir.join("tpt-shell.toml")
    }

    #[test]
    fn every_feature_section_passes_the_manifest_parser() {
        for feature in FEATURES {
            let text = format!("{BASE}\n{}", section(feature).expect("section"));
            assert!(
                tpt_shell_manifest::parse(&text).is_ok(),
                "[{feature}] does not check"
            );
        }
    }

    #[test]
    fn add_appends_a_checked_section() {
        let path = scratch("append");
        fs::write(&path, BASE).expect("write");
        assert_eq!(add("audit", &path).expect("add"), ExitCode::SUCCESS);
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("[audit]") && text.contains("store_params = false"));
        assert!(tpt_shell_manifest::parse(&text).is_ok());
        let _ = fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn add_refuses_a_second_section_and_unknown_features() {
        let path = scratch("twice");
        fs::write(&path, BASE).expect("write");
        add("kiosk", &path).expect("first");
        assert!(add("kiosk", &path).is_err());
        assert!(add("printers", &path).is_err());
        let _ = fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn an_existing_section_is_neither_duplicated_nor_changed() {
        let path = scratch("existing");
        let broken = format!("{BASE}\n[audit]\nstore_params = true\n");
        fs::write(&path, &broken).expect("write");
        assert!(add("audit", &path).is_err());
        assert_eq!(fs::read_to_string(&path).expect("read"), broken);
        let _ = fs::remove_dir_all(path.parent().expect("dir"));
    }
}
