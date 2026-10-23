// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell visual compare <dir> <dir> [dir...]`: compares the screenshots of several CI jobs.
//! The first directory is the baseline. Each screenshot is compared with the baseline's copy of
//! the same name, using `tpt_shell_test::screens`, and the result is written as one HTML page.
//! Exit 0 when every comparison passes, 1 when one fails, 2 for bad usage or I/O.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use tpt_shell_test::screens::{self, Entry, Job, Outcome};
use tpt_shell_test::visual::Comparison;

const USAGE: &str = "usage: tpt-shell visual compare <baseline-dir> <dir> [dir...]
                            [--out <report.html>] [--tolerance <0..1>] [--max-changed <0..1>]";

/// The parsed arguments of `visual compare`.
#[derive(Debug, PartialEq)]
struct CompareArgs {
    dirs: Vec<PathBuf>,
    out: PathBuf,
    tolerance: f64,
    max_changed: f64,
}

fn parse_fraction(flag: &str, value: &str) -> Result<f64, String> {
    let number: f64 = value
        .parse()
        .map_err(|_| format!("{flag} needs a number from 0 to 1, not {value:?}"))?;
    if (0.0..=1.0).contains(&number) {
        Ok(number)
    } else {
        Err(format!("{flag} must be from 0 to 1, not {value}"))
    }
}

fn parse_args(args: &[&str]) -> Result<CompareArgs, String> {
    let mut dirs = Vec::new();
    let mut out = PathBuf::from("visual-report.html");
    let mut tolerance = Comparison::default().tolerance;
    let mut max_changed = Comparison::default().max_changed_fraction;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--out" | "--tolerance" | "--max-changed" => {
                let value = *rest
                    .next()
                    .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?;
                match *arg {
                    "--out" => out = PathBuf::from(value),
                    "--tolerance" => tolerance = parse_fraction(arg, value)?,
                    _ => max_changed = parse_fraction(arg, value)?,
                }
            }
            value if !value.starts_with("--") => dirs.push(PathBuf::from(value)),
            _ => return Err(USAGE.to_string()),
        }
    }
    if dirs.len() < 2 {
        return Err(format!(
            "{USAGE}\nneed a baseline and at least one other directory"
        ));
    }
    Ok(CompareArgs {
        dirs,
        out,
        tolerance,
        max_changed,
    })
}

/// A job's name is its folder name. Two folders with the same name get a number added, so every
/// row of the report says which folder it came from.
fn job_names(dirs: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = dirs
        .iter()
        .map(|d| {
            d.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| d.display().to_string())
        })
        .collect();
    for i in 0..names.len() {
        if names[..i].contains(&names[i]) {
            names[i] = format!("{}-{}", names[i], i + 1);
        }
    }
    names
}

/// Runs `visual compare` with the arguments after the word `compare`.
///
/// Support: Full.
pub fn compare(args: &[&str]) -> Result<ExitCode, String> {
    let parsed = parse_args(args)?;
    for dir in &parsed.dirs {
        if !dir.is_dir() {
            return Err(format!("{} is not a directory", dir.display()));
        }
    }
    let names = job_names(&parsed.dirs);
    let jobs: Vec<Job> = names
        .into_iter()
        .zip(parsed.dirs.iter())
        .map(|(name, dir)| Job {
            name,
            dir: dir.clone(),
        })
        .collect();
    let cfg = Comparison {
        tolerance: parsed.tolerance,
        max_changed_fraction: parsed.max_changed,
        ..Comparison::default()
    };
    let summary = screens::compare_jobs(&jobs, 0, &cfg)?;
    let html = screens::render_html(&summary);
    fs::write(&parsed.out, html)
        .map_err(|e| format!("cannot write {}: {e}", parsed.out.display()))?;

    for entry in &summary.entries {
        println!("{}", row_line(entry));
    }
    let failed = summary.failed();
    println!(
        "{} comparison(s) against baseline {}: {failed} failed",
        summary.entries.len(),
        summary.baseline
    );
    println!("report: {}", parsed.out.display());
    Ok(ExitCode::from(summary.exit_code() as u8))
}

/// One line for a row of the summary: PASS or FAIL, the job, the screenshot name, and either the
/// share of compared pixels that changed or the reason the row could not be compared.
///
/// Support: Full.
fn row_line(entry: &Entry) -> String {
    let status = if entry.failed() { "FAIL" } else { "PASS" };
    let detail = match &entry.outcome {
        Outcome::Compared(report) => {
            let share = if report.compared == 0 {
                0.0
            } else {
                report.changed as f64 / report.compared as f64
            };
            format!("{share:.4} of compared pixels changed")
        }
        Outcome::Missing(reason) | Outcome::Failed(reason) => reason.clone(),
    };
    format!("{status} {} {}: {detail}", entry.job, entry.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A real PNG from the test crate's fixtures.
    const GRADIENT: &[u8] = include_bytes!("../../tpt-shell-test/tests/data/gradient_rgb.png");
    const PALETTE: &[u8] = include_bytes!("../../tpt-shell-test/tests/data/palette_depth2.png");

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("tpt-cli-visual-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_baseline_and_one_other_folder_are_enough() {
        let parsed = parse_args(&["a", "b", "--tolerance", "0.02"]).unwrap();
        assert_eq!(parsed.dirs, vec![PathBuf::from("a"), PathBuf::from("b")]);
        assert_eq!(parsed.tolerance, 0.02);
        assert_eq!(parsed.out, PathBuf::from("visual-report.html"));
    }

    #[test]
    fn bad_usage_is_refused() {
        assert!(parse_args(&["only-one"]).is_err());
        assert!(parse_args(&["a", "b", "--out"]).is_err());
        assert!(parse_args(&["a", "b", "--tolerance", "2"]).is_err());
        assert!(parse_args(&["a", "b", "--max-changed", "x"]).is_err());
        assert!(parse_args(&["a", "b", "--fast"]).is_err());
    }

    #[test]
    fn folders_with_the_same_name_are_told_apart() {
        let names = job_names(&[
            PathBuf::from("x/win"),
            PathBuf::from("y/win"),
            PathBuf::from("z/mac"),
        ]);
        assert_eq!(names, vec!["win", "win-2", "mac"]);
    }

    #[test]
    fn identical_screenshots_pass_and_different_ones_fail() {
        let dir = scratch("compare");
        let same_a = dir.join("same-a");
        let same_b = dir.join("same-b");
        let diff_b = dir.join("diff-b");
        for d in [&same_a, &same_b, &diff_b] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(same_a.join("home.png"), GRADIENT).unwrap();
        fs::write(same_b.join("home.png"), GRADIENT).unwrap();
        fs::write(diff_b.join("home.png"), PALETTE).unwrap();

        let out = dir.join("report.html");
        let out_str = out.to_str().unwrap();
        let pass = compare(&[
            same_a.to_str().unwrap(),
            same_b.to_str().unwrap(),
            "--out",
            out_str,
        ])
        .unwrap();
        assert_eq!(pass, ExitCode::SUCCESS);
        assert!(fs::read_to_string(&out).unwrap().contains("<html"));

        let fail = compare(&[
            same_a.to_str().unwrap(),
            diff_b.to_str().unwrap(),
            "--out",
            out_str,
        ])
        .unwrap();
        assert_eq!(fail, ExitCode::from(1));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_row_says_pass_or_fail_with_its_job_and_name() {
        let row = |outcome| Entry {
            name: "home.png".into(),
            job: "win".into(),
            outcome,
        };
        let report = |changed, passed| {
            Outcome::Compared(tpt_shell_test::visual::Report {
                compared: 200,
                changed,
                max_distance: 0.5,
                passed,
            })
        };
        assert_eq!(
            row_line(&row(report(1, true))),
            "PASS win home.png: 0.0050 of compared pixels changed"
        );
        assert!(row_line(&row(report(50, false))).starts_with("FAIL win home.png:"));
        assert_eq!(
            row_line(&row(Outcome::Missing("not in the baseline".into()))),
            "FAIL win home.png: not in the baseline"
        );
    }

    #[test]
    fn a_row_with_no_compared_pixels_reports_zero_not_nan() {
        let entry = Entry {
            name: "blank.png".into(),
            job: "mac".into(),
            outcome: Outcome::Compared(tpt_shell_test::visual::Report {
                compared: 0,
                changed: 0,
                max_distance: 0.0,
                passed: true,
            }),
        };
        assert_eq!(
            row_line(&entry),
            "PASS mac blank.png: 0.0000 of compared pixels changed"
        );
    }

    #[test]
    fn a_missing_folder_is_an_error_and_writes_nothing() {
        let dir = scratch("missing");
        let out = dir.join("report.html");
        let result = compare(&[
            dir.to_str().unwrap(),
            "no-such-folder",
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(result.is_err());
        assert!(!out.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
