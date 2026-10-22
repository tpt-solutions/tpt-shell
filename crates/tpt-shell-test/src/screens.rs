// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Merges screenshots from several CI jobs and compares them (spec §10.6).
//!
//! Each job (for example the Windows, macOS and Linux runners) writes its PNG screenshots to
//! its own directory. [`compare_jobs`] pairs the files by name, compares every job with the
//! baseline job with [`crate::visual::compare`], and returns a [`Summary`]. [`render_html`]
//! turns the summary into a single HTML page with no external resources.
//!
//! Each screenshot file is named by the test that took it, so a name present in only some
//! jobs is reported as missing rather than skipped. A file name must be ASCII letters,
//! digits, `.`, `-` or `_`, so a name cannot escape its directory.
//!
//! Support: Full for the comparison and the report. Partial: the HTML report lists results
//! and has no side-by-side or diff images yet.

use std::fs;
use std::path::{Path, PathBuf};

use crate::png;
use crate::visual::{self, Comparison, Report};

/// One CI job and the directory holding its screenshots.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub name: String,
    pub dir: PathBuf,
}

/// What happened to one screenshot name in one job.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The job's screenshot was compared with the baseline's.
    Compared(Report),
    /// The file is absent from the baseline job or from this job.
    Missing(String),
    /// The file could not be read or compared, such as a bad PNG or a different size.
    Failed(String),
}

/// One row of the summary: a screenshot name, a job, and the result against the baseline.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub job: String,
    pub outcome: Outcome,
}

impl Entry {
    /// Whether this row counts as a failure.
    ///
    /// Support: Full.
    pub fn failed(&self) -> bool {
        match &self.outcome {
            Outcome::Compared(report) => !report.passed,
            Outcome::Missing(_) | Outcome::Failed(_) => true,
        }
    }
}

/// The result of comparing several jobs.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub baseline: String,
    pub entries: Vec<Entry>,
}

impl Summary {
    /// The number of rows that failed. A CI job exits non-zero when this is above zero.
    ///
    /// Support: Full.
    pub fn failed(&self) -> usize {
        self.entries.iter().filter(|e| e.failed()).count()
    }
}

/// Compares every screenshot in every job with the baseline job, which is `jobs[baseline]`.
///
/// Support: Full.
pub fn compare_jobs(jobs: &[Job], baseline: usize, cfg: &Comparison) -> Result<Summary, String> {
    let base = jobs
        .get(baseline)
        .ok_or("the baseline job is not in the list")?;
    let mut names = Vec::new();
    for job in jobs {
        for name in screenshot_names(&job.dir)? {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names.sort();
    let mut entries = Vec::new();
    for name in &names {
        let base_path = base.dir.join(name);
        for (index, job) in jobs.iter().enumerate() {
            if index == baseline {
                continue;
            }
            let path = job.dir.join(name);
            let outcome = compare_one(&base_path, &path, cfg);
            entries.push(Entry {
                name: name.clone(),
                job: job.name.clone(),
                outcome,
            });
        }
    }
    Ok(Summary {
        baseline: base.name.clone(),
        entries,
    })
}

fn compare_one(base_path: &Path, path: &Path, cfg: &Comparison) -> Outcome {
    if !base_path.is_file() {
        return Outcome::Missing(format!("not in the baseline job ({})", base_path.display()));
    }
    if !path.is_file() {
        return Outcome::Missing("not produced by this job".into());
    }
    let read = |p: &Path| -> Result<visual::Image, String> {
        let bytes = fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        png::decode(&bytes)
    };
    let result = read(base_path)
        .and_then(|base| read(path).map(|actual| (base, actual)))
        .and_then(|(base, actual)| visual::compare(&base, &actual, cfg));
    match result {
        Ok(report) => Outcome::Compared(report),
        Err(message) => Outcome::Failed(message),
    }
}

/// The sorted names of the PNG files in a directory. Other files are ignored. A name that
/// is not a plain file name is an error, so a hostile name cannot reach another directory.
fn screenshot_names(dir: &Path) -> Result<Vec<String>, String> {
    let read = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut names = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".png") {
            continue;
        }
        if !is_plain_name(&name) {
            return Err(format!("`{name}` is not a plain file name; rename it"));
        }
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Renders the summary as one HTML page. All text is escaped, and the page loads nothing.
///
/// Support: Partial. The page lists results only; it has no side-by-side or diff images yet.
pub fn render_html(summary: &Summary) -> String {
    let mut rows = String::new();
    for e in &summary.entries {
        let (status, detail) = match &e.outcome {
            Outcome::Compared(r) => (
                if r.passed { "pass" } else { "fail" },
                format!(
                    "{:.4}% changed, largest distance {:.3}, {} of {} pixels compared",
                    r.changed_fraction() * 100.0,
                    r.max_distance,
                    r.changed,
                    r.compared
                ),
            ),
            Outcome::Missing(m) => ("fail", m.clone()),
            Outcome::Failed(m) => ("fail", m.clone()),
        };
        rows.push_str(&format!(
            "<tr class=\"{status}\"><td>{}</td><td>{}</td><td>{status}</td><td>{}</td></tr>\n",
            escape(&e.name),
            escape(&e.job),
            escape(&detail)
        ));
    }
    let failed = summary.failed();
    let verdict = if failed == 0 {
        "All screenshots are within their thresholds.".to_string()
    } else {
        format!("{failed} screenshot comparison(s) failed.")
    };
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>Visual compare</title>\n\
<style>\n\
:root {{ color-scheme: light dark; --fg: #1d1d1f; --bg: #ffffff; --pass: #1e7b34; --fail: #b3261e; }}\n\
@media (prefers-color-scheme: dark) {{ :root {{ --fg: #f2f2f2; --bg: #161618; --pass: #6ad586; --fail: #ff8a80; }} }}\n\
body {{ font-family: system-ui, sans-serif; color: var(--fg); background: var(--bg); margin: 16px; }}\n\
table {{ border-collapse: collapse; width: 100%; }}\n\
th, td {{ text-align: left; padding: 6px 8px; border-bottom: 1px solid #8884; vertical-align: top; }}\n\
tr.pass td:nth-child(3) {{ color: var(--pass); font-weight: 600; }}\n\
tr.fail td:nth-child(3) {{ color: var(--fail); font-weight: 600; }}\n\
</style></head><body>\n\
<h1>Visual compare</h1>\n\
<p>Baseline job: {}. {}</p>\n\
<table><thead><tr><th>Screenshot</th><th>Job</th><th>Result</th><th>Detail</th></tr></thead>\n\
<tbody>\n{rows}</tbody></table>\n</body></html>\n",
        escape(&summary.baseline),
        escape(&verdict),
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRADIENT: &[u8] = include_bytes!("../tests/data/gradient_rgb.png");
    const STORED: &[u8] = include_bytes!("../tests/data/gradient_stored.png");

    /// A fresh directory under the target dir, unique per test name.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-shell-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn identical_jobs_pass_and_stored_and_compressed_match() {
        let root = scratch("identical");
        let (a, b) = (root.join("win"), root.join("mac"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("home.png"), GRADIENT).unwrap();
        fs::write(b.join("home.png"), STORED).unwrap();
        let jobs = [
            Job {
                name: "windows".into(),
                dir: a,
            },
            Job {
                name: "macos".into(),
                dir: b,
            },
        ];
        let summary = compare_jobs(&jobs, 0, &Comparison::default()).unwrap();
        assert_eq!(summary.entries.len(), 1);
        assert_eq!(summary.failed(), 0);
        let html = render_html(&summary);
        assert!(html.contains("All screenshots are within their thresholds."));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_file_and_a_changed_file_both_fail() {
        let root = scratch("missing");
        let (a, b) = (root.join("win"), root.join("linux"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("home.png"), GRADIENT).unwrap();
        fs::write(a.join("only-windows.png"), GRADIENT).unwrap();
        fs::write(b.join("home.png"), GRADIENT).unwrap();
        let jobs = [
            Job {
                name: "windows".into(),
                dir: a,
            },
            Job {
                name: "linux".into(),
                dir: b,
            },
        ];
        let summary = compare_jobs(&jobs, 0, &Comparison::default()).unwrap();
        assert_eq!(summary.entries.len(), 2);
        let missing = summary
            .entries
            .iter()
            .find(|e| e.name == "only-windows.png")
            .unwrap();
        assert!(matches!(missing.outcome, Outcome::Missing(_)));
        assert_eq!(summary.failed(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_size_mismatch_is_a_failure_not_a_panic() {
        let root = scratch("different");
        let (a, b) = (root.join("win"), root.join("mac"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("home.png"), GRADIENT).unwrap();
        fs::write(
            b.join("home.png"),
            include_bytes!("../tests/data/grey_alpha.png"),
        )
        .unwrap();
        let jobs = [
            Job {
                name: "windows".into(),
                dir: a,
            },
            Job {
                name: "macos".into(),
                dir: b,
            },
        ];
        let summary = compare_jobs(&jobs, 0, &Comparison::default()).unwrap();
        // Different sizes: reported as a failure, not a panic.
        assert!(matches!(summary.entries[0].outcome, Outcome::Failed(_)));
        assert_eq!(summary.failed(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn names_that_are_not_plain_are_refused() {
        assert!(is_plain_name("home-1.png"));
        assert!(!is_plain_name("../home.png"));
        assert!(!is_plain_name("a b.png"));
        assert!(!is_plain_name(""));
    }

    #[test]
    fn html_escapes_names_and_loads_nothing() {
        let summary = Summary {
            baseline: "<win>".into(),
            entries: vec![Entry {
                name: "x.png".into(),
                job: "linux".into(),
                outcome: Outcome::Failed("<script>alert(1)</script>".into()),
            }],
        };
        let html = render_html(&summary);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&lt;win&gt;"));
        assert!(!html.contains("src="));
        assert!(!html.contains("http"));
        assert_eq!(summary.failed(), 1);
    }
}
