// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Licence notices (spec §10.8): `licenses.html` (also embedded at `app://__tpt/licenses`) and
//! `THIRD_PARTY_NOTICES`. The texts come from the licence files shipped in each crate's source
//! directory, located through `cargo metadata`. Output is deterministic.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{components_from_metadata, App, Component};

/// The app path under which `licenses.html` is served. The facade and platform crates embed it.
///
/// Support: Partial. The constant names the path where the notices are served. The app layer that
/// serves them is not in this crate, and no test here reads the path.
pub const LICENSES_PATH: &str = "__tpt/licenses";

/// Licence files larger than this are skipped, so one file cannot bloat the notices.
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// One licence file shipped with a crate.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseFile {
    pub name: String,
    pub text: String,
}

/// A third-party crate and its licence files.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub component: Component,
    pub files: Vec<LicenseFile>,
}

/// Builds one notice per third-party crate in `cargo metadata` output, reading each crate's
/// licence files from its source directory. A crate whose directory is missing gets no files.
///
/// Support: Full.
pub fn notices_from_metadata(json_text: &str) -> Result<Vec<Notice>, String> {
    let components = components_from_metadata(json_text)?;
    let doc: Value =
        serde_json::from_str(json_text).map_err(|e| format!("metadata is not JSON: {e}"))?;
    let mut dirs: BTreeMap<(String, String), PathBuf> = BTreeMap::new();
    for package in doc
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if package.get("source").is_none_or(Value::is_null) {
            continue;
        }
        let (Some(name), Some(version), Some(manifest)) = (
            package.get("name").and_then(Value::as_str),
            package.get("version").and_then(Value::as_str),
            package.get("manifest_path").and_then(Value::as_str),
        ) else {
            continue;
        };
        if let Some(dir) = Path::new(manifest).parent() {
            dirs.entry((name.to_string(), version.to_string()))
                .or_insert_with(|| dir.to_path_buf());
        }
    }
    Ok(components
        .into_iter()
        .map(|component| {
            let files = dirs
                .get(&(component.name.clone(), component.version.clone()))
                .map(|dir| read_license_files(dir))
                .unwrap_or_default();
            Notice { component, files }
        })
        .collect())
}

/// The licence files in a crate directory: names starting with LICENSE, LICENCE, COPYING,
/// NOTICE or UNLICENSE (any case), in name order. Non-UTF-8 bytes are replaced.
///
/// Support: Full.
pub fn read_license_files(dir: &Path) -> Vec<LicenseFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            let wanted = ["license", "licence", "copying", "notice", "unlicense"]
                .iter()
                .any(|p| lower.starts_with(p));
            let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
            (wanted && is_file).then(|| (name, e.path()))
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|(name, path)| {
            let size = std::fs::metadata(&path).ok()?.len();
            if size > MAX_FILE_BYTES {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            Some(LicenseFile {
                name,
                text: String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
            })
        })
        .collect()
}

/// Escapes text for HTML element content and quoted attributes.
///
/// Support: Full.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// The anchor id of a crate's section, such as `serde-1-0-200`. Only ASCII letters, digits and
/// hyphens are kept.
fn anchor_id(c: &Component) -> String {
    format!("{}-{}", c.name, c.version)
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

/// The page served at `app://__tpt/licenses`: a table of crates and licences, then each
/// crate's licence files.
///
/// Support: Full.
pub fn licenses_html(app: &App, notices: &[Notice]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str(&format!(
        "<title>Licences for {}</title>\n",
        escape_html(&format!("{} {}", app.name, app.version))
    ));
    out.push_str(
        "<style>body{font-family:system-ui,sans-serif;margin:16px;max-width:960px;color:#111;background:#fff}\
         table{border-collapse:collapse}td,th{border:1px solid #bbb;padding:4px 8px;text-align:left;vertical-align:top}\
         pre{white-space:pre-wrap;background:#f4f4f4;padding:8px;font-size:12px;overflow-x:auto}\
         @media (prefers-color-scheme: dark){body{color:#eee;background:#161616}pre{background:#222}td,th{border-color:#555}}</style>\n",
    );
    out.push_str("</head>\n<body>\n");
    out.push_str(&format!(
        "<h1>Third-party licences for {}</h1>\n",
        escape_html(&format!("{} {}", app.name, app.version))
    ));
    out.push_str(&format!(
        "<p>{} third-party crates. Generated by tpt-shell from cargo metadata.</p>\n",
        notices.len()
    ));
    out.push_str("<table>\n<tr><th>Crate</th><th>Version</th><th>Licence</th></tr>\n");
    for notice in notices {
        let c = &notice.component;
        out.push_str(&format!(
            "<tr><td><a href=\"#{}\">{}</a></td><td>{}</td><td>{}</td></tr>\n",
            anchor_id(c),
            escape_html(&c.name),
            escape_html(&c.version),
            escape_html(c.license.as_deref().unwrap_or("not declared"))
        ));
    }
    out.push_str("</table>\n");
    for notice in notices {
        let c = &notice.component;
        out.push_str(&format!(
            "<section id=\"{}\">\n<h2>{} {}</h2>\n",
            anchor_id(c),
            escape_html(&c.name),
            escape_html(&c.version)
        ));
        out.push_str(&format!(
            "<p>Licence: {}</p>\n",
            escape_html(c.license.as_deref().unwrap_or("not declared"))
        ));
        if notice.files.is_empty() {
            out.push_str("<p>No licence file is shipped with this crate.</p>\n");
        }
        for file in &notice.files {
            out.push_str(&format!(
                "<h3>{}</h3>\n<pre>{}</pre>\n",
                escape_html(&file.name),
                escape_html(&file.text)
            ));
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// The plain-text `THIRD_PARTY_NOTICES` file: a header, then each crate with its licence and
/// licence files, separated by rules.
///
/// Support: Full.
pub fn third_party_notices(app: &App, notices: &[Notice]) -> String {
    let rule = "-".repeat(72);
    let mut out = format!(
        "THIRD-PARTY NOTICES for {} {}\n\nThis product includes the following third-party crates.\nEach crate is listed with its declared licence and the licence files it ships.\n\n",
        app.name, app.version
    );
    for notice in notices {
        let c = &notice.component;
        out.push_str(&rule);
        out.push('\n');
        out.push_str(&format!(
            "{} {}\nLicence: {}\n",
            c.name,
            c.version,
            c.license.as_deref().unwrap_or("not declared")
        ));
        if let Some(repo) = &c.repository {
            out.push_str(&format!("Source: {repo}\n"));
        }
        if notice.files.is_empty() {
            out.push_str("No licence file is shipped with this crate.\n");
        }
        for file in &notice.files {
            out.push_str(&format!(
                "\n--- {} ---\n{}\n",
                file.name,
                file.text.trim_end()
            ));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        App {
            name: "demo".into(),
            version: "1.0.0".into(),
        }
    }

    fn notice(name: &str, license: Option<&str>, text: Option<&str>) -> Notice {
        Notice {
            component: Component {
                name: name.into(),
                version: "0.1.0".into(),
                license: license.map(str::to_string),
                repository: Some("https://example.invalid/<repo>".into()),
            },
            files: text
                .map(|t| {
                    vec![LicenseFile {
                        name: "LICENSE-MIT".into(),
                        text: t.into(),
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn html_escapes_crate_text_and_licence_files() {
        let html = licenses_html(
            &app(),
            &[notice("<script>x</script>", Some("MIT"), Some("a & b < c"))],
        );
        assert!(!html.contains("<script>x"));
        assert!(html.contains("&lt;script&gt;x&lt;/script&gt;"));
        assert!(html.contains("a &amp; b &lt; c"));
        assert!(html.starts_with("<!doctype html>"));
    }

    #[test]
    fn a_crate_without_a_licence_file_says_so() {
        let html = licenses_html(&app(), &[notice("bare", None, None)]);
        assert!(html.contains("No licence file is shipped with this crate."));
        assert!(html.contains("not declared"));
        let text = third_party_notices(&app(), &[notice("bare", None, None)]);
        assert!(text.contains("Licence: not declared"));
    }

    #[test]
    fn third_party_notices_lists_each_crate_with_its_text() {
        let text = third_party_notices(
            &app(),
            &[notice(
                "alpha",
                Some("MIT"),
                Some("Permission is hereby granted\n"),
            )],
        );
        assert!(text.starts_with("THIRD-PARTY NOTICES for demo 1.0.0"));
        assert!(text.contains("alpha 0.1.0\nLicence: MIT\nSource: https://example.invalid/<repo>"));
        assert!(text.contains("--- LICENSE-MIT ---\nPermission is hereby granted\n"));
    }

    #[test]
    fn output_is_deterministic() {
        let notices = vec![notice("alpha", Some("MIT"), Some("text"))];
        assert_eq!(
            licenses_html(&app(), &notices),
            licenses_html(&app(), &notices)
        );
    }

    #[test]
    fn license_files_are_found_by_name_and_read() {
        let dir = std::env::temp_dir().join(format!(
            "tpt-notices-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("LICENSE-MIT"), "mit text\r\n").unwrap();
        std::fs::write(dir.join("COPYING"), "copy").unwrap();
        std::fs::write(dir.join("README.md"), "not a licence").unwrap();
        let files = read_license_files(&dir);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["COPYING", "LICENSE-MIT"]);
        assert_eq!(files[1].text, "mit text\n");
        assert!(read_license_files(&dir.join("absent")).is_empty());
    }

    #[test]
    fn notices_come_from_the_metadata_directories() {
        let dir = std::env::temp_dir().join(format!(
            "tpt-notices-meta-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("LICENSE"), "shipped").unwrap();
        let manifest = dir
            .join("Cargo.toml")
            .to_string_lossy()
            .replace('\\', "\\\\");
        let metadata = format!(
            r#"{{"packages":[{{"name":"dep","version":"0.1.0","source":"registry+x","license":"MIT","repository":null,"manifest_path":"{manifest}"}}]}}"#
        );
        let notices = notices_from_metadata(&metadata).unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].files[0].text, "shipped");
    }
}
