// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Merges the conformance result files of every OS into the compatibility report.
//!
//! usage: conformance-report <results-directory> <output-directory>
//!
//! Reads each `*.json` file in the results directory (one per OS, written by the suite) and
//! writes `compatibility.md` (the published report) and `compatibility.json` (the merged data)
//! to the output directory. The CLI may later run this as `tpt-shell conformance report`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt_shell_conformance::{render_markdown, RunReport};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: conformance-report <results-directory> <output-directory>");
        return ExitCode::from(2);
    };
    match run(Path::new(input), Path::new(output)) {
        Ok(count) => {
            println!("wrote the report from {count} run(s) to {output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("conformance-report: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(input: &Path, output: &Path) -> Result<usize, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(input)
        .map_err(|e| format!("cannot read {}: {e}", input.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no result files (*.json) in {}", input.display()));
    }

    let mut runs = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not JSON: {e}", file.display()))?;
        let run = RunReport::from_json(&value)
            .map_err(|e| format!("{} is not a result file: {e}", file.display()))?;
        runs.push(run);
    }
    runs.sort_by(|a, b| a.platform.cmp(&b.platform));

    std::fs::create_dir_all(output)
        .map_err(|e| format!("cannot create {}: {e}", output.display()))?;
    let markdown = render_markdown(&runs);
    std::fs::write(output.join("compatibility.md"), markdown)
        .map_err(|e| format!("cannot write the markdown report: {e}"))?;
    let merged = serde_json::json!({
        "runs": runs.iter().map(RunReport::to_json).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
    std::fs::write(output.join("compatibility.json"), text)
        .map_err(|e| format!("cannot write the JSON report: {e}"))?;
    Ok(runs.len())
}
