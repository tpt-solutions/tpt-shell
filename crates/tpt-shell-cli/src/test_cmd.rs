// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell test collect <run-dir> <out-dir> [--test <name>]` (spec §10.6): gathers the
//! artifacts of one test-driver run into an output folder, one folder per test, and writes the
//! `index.json` that CI uploads.
//!
//! A test run writes its screenshots, replays and logs into `<run-dir>/artifacts/`. The
//! protocol files of the run (`req-*.json` and `res-*.json`) are not artifacts and are left
//! out. The test's name is `--test`, or the run folder's own name when that is not given. Each
//! file's kind comes from its extension: `.png` is a screenshot, `.tptreplay` a replay, `.log`
//! or `.txt` a log, and anything else is other.
//!
//! Running the tests is not done here: a test run sets `TPT_TEST_RUN` and runs the app's
//! `test-driver` build, for example `cargo test --features test-driver`. Then this command
//! collects what the run wrote.
//!
//! Support: Full for the collection. Partial overall: it collects only what a run wrote to
//! `artifacts/`, and it does not start tests.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tpt_shell_test::artifacts::{is_plain, Collector, Kind};

const USAGE: &str = "usage: tpt-shell test collect <run-dir> <out-dir> [--test <name>]";

/// The kind of an artifact, from its extension.
fn kind_of(file: &str) -> Kind {
    match Path::new(file).extension().and_then(|e| e.to_str()) {
        Some("png") => Kind::Screenshot,
        Some("tptreplay") => Kind::Replay,
        Some("log") | Some("txt") => Kind::Log,
        _ => Kind::Other,
    }
}

/// Runs `test collect` with the arguments after the word `collect`.
///
/// Support: Full.
pub fn collect(args: &[&str]) -> Result<ExitCode, String> {
    let mut positional = Vec::new();
    let mut test: Option<&str> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if *arg == "--test" {
            test = Some(
                *rest
                    .next()
                    .ok_or_else(|| format!("--test needs a name\n{USAGE}"))?,
            );
        } else if arg.starts_with("--") {
            return Err(USAGE.to_string());
        } else {
            positional.push(*arg);
        }
    }
    let [run, out] = positional[..] else {
        return Err(USAGE.to_string());
    };
    let run = Path::new(run);
    let artifacts = run.join("artifacts");
    if !artifacts.is_dir() {
        return Err(format!("{} has no artifacts folder", run.display()));
    }
    let test_name = match test {
        Some(name) => name.to_string(),
        None => run
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("cannot name the test after {}", run.display()))?,
    };
    if !is_plain(&test_name) {
        return Err(format!("`{test_name}` is not a plain test name"));
    }

    let mut collector = Collector::new(Path::new(out))?;
    let mut files: Vec<_> = fs::read_dir(&artifacts)
        .map_err(|e| format!("cannot read {}: {e}", artifacts.display()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_file())
        .collect();
    files.sort_by_key(|entry| entry.file_name());
    for entry in files {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_plain(&name) {
            return Err(format!("artifact `{name}` is not a plain file name"));
        }
        let bytes = fs::read(entry.path()).map_err(|e| format!("cannot read {name}: {e}"))?;
        collector.add(&test_name, kind_of(&name), &name, &bytes)?;
    }
    let count = collector.entries().len();
    let index = collector.finish()?;
    println!("collected {count} artifact(s) for {test_name}");
    println!("index: {}", index.display());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tpt-cli-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn artifacts_are_collected_with_their_kinds_and_the_protocol_files_are_left_out() {
        let run = temp("run").join("login-flow");
        let out = temp("out");
        let _ = fs::remove_dir_all(temp("run"));
        let _ = fs::remove_dir_all(&out);
        fs::create_dir_all(run.join("artifacts")).unwrap();
        fs::write(run.join("artifacts").join("after-input.png"), b"\x89PNG").unwrap();
        fs::write(run.join("artifacts").join("session.tptreplay"), b"x").unwrap();
        fs::write(run.join("artifacts").join("driver.log"), b"ok").unwrap();
        fs::write(run.join("req-00000001.json"), b"{}").unwrap();

        assert_eq!(
            collect(&[run.to_str().unwrap(), out.to_str().unwrap()]).expect("collects"),
            ExitCode::SUCCESS
        );
        let index: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(out.join("index.json")).unwrap()).unwrap();
        let files = index["files"].as_array().unwrap();
        let kinds: Vec<(String, String)> = files
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["kind"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(kinds.len(), 3, "{kinds:?}");
        assert!(kinds.contains(&("login-flow/after-input.png".into(), "screenshot".into())));
        assert!(kinds.contains(&("login-flow/session.tptreplay".into(), "replay".into())));
        assert!(kinds.contains(&("login-flow/driver.log".into(), "log".into())));
        assert!(out.join("login-flow").join("after-input.png").is_file());
        let _ = fs::remove_dir_all(temp("run"));
        let _ = fs::remove_dir_all(&out);
    }

    #[test]
    fn a_missing_artifacts_folder_or_a_bad_usage_is_refused() {
        assert!(collect(&["nope", "out"]).is_err());
        assert!(collect(&["only-one"]).is_err());
        assert!(collect(&["a", "b", "--bogus"]).is_err());
    }
}
