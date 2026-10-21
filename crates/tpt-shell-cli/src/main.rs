// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell`: explain error codes, check manifests, and create starter projects.
//!
//! Exit codes: 0 success, 1 the checked input has problems, 2 bad usage or I/O failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt_shell_audit::AuditError;

mod dev;
use tpt_shell_errors::lookup;

const USAGE: &str = "usage:
  tpt-shell explain <TS####>
  tpt-shell dev <directory> [--port <port>]   serve files on 127.0.0.1 and report changes
  tpt-shell manifest check <tpt-shell.toml>
  tpt-shell contract diff <old-contract.json> <new-contract.json>
  tpt-shell contract check <new-contract.json> --baseline <old-contract.json>
  tpt-shell new vanilla <directory>
  tpt-shell new typescript <directory>
  tpt-shell doctor
  tpt-shell compliance <output-directory>
  tpt-shell audit verify <audit.jsonl> [--key <public-key-file>]
  tpt-shell audit export <audit.jsonl> --format csv|jsonl
  tpt-shell audit view <audit.jsonl>
  tpt-shell install-service <name> <program>     start the program at sign-in (Windows)
  tpt-shell uninstall-service <name>             remove that sign-in entry (Windows)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

/// Drops the `\\?\` prefix that Windows adds when a path is canonicalized, so a path printed
/// for people reads as the path they typed.
fn without_verbatim_prefix(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

/// Serves `dir` on the loopback address at `port`, reports file changes, and runs until stopped.
fn dev_server(dir: &Path, port: u16) -> Result<ExitCode, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let root =
        fs::canonicalize(dir).map_err(|e| format!("cannot resolve {}: {e}", dir.display()))?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    dev::watch(root.clone(), std::time::Duration::from_millis(500));
    dev::serve(&root, addr, |bound| {
        println!(
            "serving {} at http://{bound}/ (Ctrl+C to stop)",
            without_verbatim_prefix(&root.display().to_string())
        );
    })?;
    Ok(ExitCode::SUCCESS)
}

/// Runs one command. `Err` means bad usage or an I/O failure (exit 2).
fn run(args: &[&str]) -> Result<ExitCode, String> {
    match args {
        ["explain", code] => Ok(explain(code)),
        ["manifest", "check", path] => manifest_check(Path::new(path)),
        ["contract", "diff", old, new] => contract_compare(Path::new(old), Path::new(new), false),
        ["contract", "check", new, "--baseline", old] => {
            contract_compare(Path::new(old), Path::new(new), true)
        }
        ["dev", dir] => dev_server(Path::new(dir), 8080),
        ["dev", dir, "--port", port] => {
            let port: u16 = port
                .parse()
                .map_err(|_| format!("{port:?} is not a port number"))?;
            dev_server(Path::new(dir), port)
        }
        ["new", "vanilla", dir] => new_vanilla(Path::new(dir)).map(|()| ExitCode::SUCCESS),
        ["new", "typescript", dir] => new_typescript(Path::new(dir)).map(|()| ExitCode::SUCCESS),
        ["doctor"] => Ok(doctor()),
        ["compliance", dir] => compliance(Path::new(dir)),
        ["audit", "verify", path] => audit_verify(Path::new(path), None),
        ["audit", "verify", path, "--key", key] => {
            audit_verify(Path::new(path), Some(Path::new(key)))
        }
        ["audit", "export", path, "--format", "csv"] => audit_export_csv(Path::new(path)),
        ["audit", "export", path, "--format", "jsonl"] => audit_export_jsonl(Path::new(path)),
        ["audit", "view", path] => audit_view(Path::new(path)),
        ["install-service", name, program] => install_service(name, Path::new(program)),
        ["uninstall-service", name] => uninstall_service(name),
        _ => Err(USAGE.to_string()),
    }
}

/// Lists how `new` differs from `old`. With `check`, lists only breaking changes and exits 1
/// when there is one, so a build can refuse a contract that breaks its callers.
fn contract_compare(old: &Path, new: &Path, check: bool) -> Result<ExitCode, String> {
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
    };
    let changes = tpt_shell_ipc::contract_diff::diff(&read(old)?, &read(new)?)?;
    if changes.is_empty() {
        println!("no changes");
        return Ok(ExitCode::SUCCESS);
    }
    let breaking = changes.iter().any(|c| c.breaking);
    for change in changes.iter().filter(|c| c.breaking || !check) {
        let tag = if change.breaking {
            "breaking"
        } else {
            "compatible"
        };
        println!("{tag}  {}: {}", change.path, change.message);
    }
    if !check {
        return Ok(ExitCode::SUCCESS);
    }
    if breaking {
        Ok(ExitCode::from(1))
    } else {
        println!("no breaking changes");
        Ok(ExitCode::SUCCESS)
    }
}

/// Adds a sign-in entry that starts `program` for the current user. The program must exist.
fn install_service(name: &str, program: &Path) -> Result<ExitCode, String> {
    if !program.is_file() {
        return Err(format!("{} is not a file", program.display()));
    }
    let absolute = fs::canonicalize(program)
        .map_err(|e| format!("cannot resolve {}: {e}", program.display()))?;
    startup_entry(name, Some(&absolute))
}

/// Removes the sign-in entry `name`. Removing one that is not there is not an error.
fn uninstall_service(name: &str) -> Result<ExitCode, String> {
    startup_entry(name, None)
}

#[cfg(windows)]
fn startup_entry(name: &str, program: Option<&Path>) -> Result<ExitCode, String> {
    use tpt_shell_platform_windows::startup;
    match program {
        Some(program) => {
            // The path is quoted, so spaces in it survive the shell's command line handling.
            let command = format!("\"{}\"", program.display());
            startup::enable(name, &command)?;
            println!("{name} will start at sign-in: {command}");
        }
        None => {
            startup::disable(name)?;
            println!("{name} will no longer start at sign-in");
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(windows))]
fn startup_entry(_name: &str, _program: Option<&Path>) -> Result<ExitCode, String> {
    Err("sign-in entries are only supported on Windows for now".into())
}

/// A broken chain is a problem in the checked input (exit 1). Anything else is an I/O failure
/// (exit 2), reported through the `Err` path.
fn audit_failure(error: AuditError) -> Result<ExitCode, String> {
    if let AuditError::Chain { .. } = error {
        eprintln!("{error}");
        return Ok(ExitCode::from(1));
    }
    Err(error.to_string())
}

/// Checks the hash chain, and the checkpoint signatures when a public key is given.
fn audit_verify(path: &Path, key: Option<&Path>) -> Result<ExitCode, String> {
    let summary = match key {
        None => tpt_shell_audit::verify_file(path),
        Some(key) => {
            let text = fs::read_to_string(key)
                .map_err(|e| format!("cannot read {}: {e}", key.display()))?;
            let key = tpt_shell_audit::verifying_key_from_hex(&text).ok_or_else(|| {
                format!("{} is not a 64-digit hex Ed25519 public key", key.display())
            })?;
            tpt_shell_audit::verify_file_with_key(path, &key)
        }
    };
    match summary {
        Ok(summary) => {
            println!(
                "ok: {} records, {} checkpoints, last hash {}",
                summary.records, summary.checkpoints, summary.last_hash
            );
            if key.is_none() && summary.checkpoints > 0 {
                println!("note: checkpoint signatures were not checked (pass --key)");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => audit_failure(error),
    }
}

/// Prints the verified log as CSV (see [`tpt_shell_audit::export_csv`]).
fn audit_export_csv(path: &Path) -> Result<ExitCode, String> {
    match tpt_shell_audit::export_csv(path) {
        Ok(csv) => {
            print!("{csv}");
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => audit_failure(error),
    }
}

/// Prints the verified log as JSON Lines. The file already is JSON Lines, so this copies it.
fn audit_export_jsonl(path: &Path) -> Result<ExitCode, String> {
    if let Err(error) = tpt_shell_audit::verify_file(path) {
        return audit_failure(error);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    print!("{text}");
    Ok(ExitCode::SUCCESS)
}

/// Prints the verified log as a short table: sequence, time, actor, command and outcome.
fn audit_view(path: &Path) -> Result<ExitCode, String> {
    if let Err(error) = tpt_shell_audit::verify_file(path) {
        return audit_failure(error);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    println!(
        "{:>6}  {:>13}  {:<12}  {:<24}  outcome",
        "seq", "unix ms", "actor", "command"
    );
    for line in text.lines() {
        let record: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let field = |name: &str| match &record[name] {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        println!(
            "{:>6}  {:>13}  {:<12}  {:<24}  {}",
            field("seq"),
            field("wall_unix_ms"),
            field("actor"),
            field("command"),
            field("outcome")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn explain(code: &str) -> ExitCode {
    match lookup(code) {
        Some(entry) => {
            println!("{}: {}", entry.code, entry.title);
            println!();
            println!("{}", entry.explanation);
            println!();
            println!("Fix: {}", entry.fix);
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("no entry for `{code}`");
            ExitCode::from(1)
        }
    }
}

fn manifest_check(path: &Path) -> Result<ExitCode, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    match tpt_shell_manifest::parse(&text) {
        Ok(manifest) => {
            println!(
                "{}: ok ({} window(s), {} capability grant(s))",
                path.display(),
                manifest.windows.len(),
                manifest.capabilities.len()
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            println!("{}: {e}", path.display());
            if let Some(entry) = lookup(e.code) {
                println!("  {}", entry.fix);
            }
            Ok(ExitCode::from(1))
        }
    }
}

/// The starter manifest for `new vanilla`.
const VANILLA_MANIFEST: &str = r#"[app]
name = "my-app"
version = "0.1.0"

[[windows]]
name = "main"
title = "My App"
width = 960
height = 640

[capabilities]
grant = []
"#;

/// The starter page for `new vanilla`. It has no external scripts or styles.
const VANILLA_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>My App</title>
  <style>
    :root { color-scheme: light dark; }
    body { font-family: system-ui, sans-serif; margin: 2rem; }
  </style>
</head>
<body>
  <h1>My App</h1>
  <p>Edit <code>index.html</code> to change this page.</p>
</body>
</html>
"#;

fn new_vanilla(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        let mut entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if entries.next().is_some() {
            return Err(format!(
                "{} is not empty; choose a new directory",
                dir.display()
            ));
        }
    } else {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    write(&dir.join("tpt-shell.toml"), VANILLA_MANIFEST)?;
    write(&dir.join("index.html"), VANILLA_PAGE)?;
    println!("created {}", dir.display());
    Ok(())
}

fn write(path: &PathBuf, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tpt-cli-{name}-{}-{n}", std::process::id()))
    }

    #[test]
    fn every_manifest_code_is_explained() {
        // The codes the manifest parser can produce must all be in the registry.
        for code in [
            tpt_shell_manifest::codes::SYNTAX,
            tpt_shell_manifest::codes::UNKNOWN_KEY,
            tpt_shell_manifest::codes::INVALID_VALUE,
            tpt_shell_manifest::codes::MISSING_KEY,
        ] {
            assert!(lookup(code).is_some(), "{code} has no registry entry");
        }
    }

    #[test]
    fn the_verbatim_prefix_is_dropped_only_when_present() {
        assert_eq!(without_verbatim_prefix(r"\\?\C:\app"), r"C:\app");
        assert_eq!(without_verbatim_prefix(r"C:\app"), r"C:\app");
    }

    #[test]
    fn new_vanilla_writes_a_manifest_that_checks_clean() {
        let dir = scratch("new");
        new_vanilla(&dir).unwrap();
        assert!(dir.join("index.html").exists());
        let text = fs::read_to_string(dir.join("tpt-shell.toml")).unwrap();
        assert!(tpt_shell_manifest::parse(&text).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_vanilla_refuses_a_non_empty_directory() {
        let dir = scratch("busy");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("keep.txt"), "x").unwrap();
        assert!(new_vanilla(&dir).is_err());
        assert!(!dir.join("tpt-shell.toml").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_check_reports_problems_with_exit_code_one() {
        let path = scratch("bad").with_extension("toml");
        fs::write(&path, "[app]\nname = \"has space\"\n").unwrap();
        let code = manifest_check(&path).unwrap();
        assert_eq!(code, ExitCode::from(1));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn unknown_commands_are_usage_errors() {
        assert!(run(&["bogus"]).is_err());
        assert!(run(&["explain"]).is_err());
    }

    /// Writes a three-record audit log and returns its path.
    fn sample_audit_log(name: &str) -> PathBuf {
        use tpt_shell_audit::{AuditLog, Entry, Outcome, Policy};
        let path = scratch(name);
        let mut log = AuditLog::open(&path, Policy::default()).unwrap();
        for n in 0..3 {
            log.append(&Entry {
                wall_unix_ms: 1_700_000_000_000 + n,
                mono_ns: n,
                actor: "page".into(),
                command: format!("cmd_{n}"),
                capability: None,
                params: serde_json::json!({ "n": n }),
                outcome: Outcome::Ok,
                duration_us: 1,
            })
            .unwrap();
        }
        path
    }

    #[test]
    fn audit_commands_run_on_a_good_log() {
        let path = sample_audit_log("good");
        let path = path.to_str().unwrap();
        assert!(run(&["audit", "verify", path]).is_ok());
        assert!(run(&["audit", "export", path, "--format", "csv"]).is_ok());
        assert!(run(&["audit", "export", path, "--format", "jsonl"]).is_ok());
        assert!(run(&["audit", "view", path]).is_ok());
    }

    #[test]
    fn a_tampered_log_is_a_problem_not_an_io_failure() {
        let path = sample_audit_log("tampered");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replacen("cmd_1", "cmd_x", 1)).unwrap();
        let path = path.to_str().unwrap();
        assert!(run(&["audit", "verify", path]).is_ok());
        assert!(run(&["audit", "view", path]).is_ok());
    }

    #[test]
    fn a_missing_log_is_an_io_failure() {
        assert!(run(&["audit", "verify", "no-such-log.jsonl"]).is_err());
    }

    #[test]
    fn bad_audit_usage_is_refused() {
        assert!(run(&["audit"]).is_err());
        assert!(run(&["audit", "export", "x.jsonl", "--format", "xlsx"]).is_err());
        assert!(run(&["audit", "verify", "x.jsonl", "--key"]).is_err());
    }
}

/// Install key of the WebView2 Evergreen runtime (the same GUID on every machine).
const WEBVIEW2_CLIENT: &str =
    r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// Reports what the build and run need. Exit 0 when the required parts are present, 1 when
/// the WebView2 runtime is missing.
fn doctor() -> ExitCode {
    let mut ok = true;
    println!("platform: {}", std::env::consts::OS);
    match tool_version("rustc", &["--version"]) {
        Some(v) => println!("rustc: {v}"),
        None => println!("rustc: not found (install Rust from rustup.rs)"),
    }
    match tool_version("cargo", &["deny", "--version"]) {
        Some(v) => println!("cargo-deny: {v}"),
        None => println!("cargo-deny: not found (needed for the licence and advisory gate)"),
    }
    match webview2_version() {
        Some(v) => println!("WebView2 runtime: {v}"),
        None => {
            ok = false;
            println!("WebView2 runtime: not found (install the Evergreen runtime from Microsoft)");
        }
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// First line of a tool's output, or `None` when the tool does not run.
fn tool_version(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_string())
}

/// The runtime version, from the per-machine or per-user install key. Windows only.
fn webview2_version() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    for root in [
        r"HKLM\SOFTWARE\WOW6432Node",
        r"HKLM\SOFTWARE",
        r"HKCU\SOFTWARE",
    ] {
        let key = format!(r"{root}\{WEBVIEW2_CLIENT}");
        let Ok(out) = std::process::Command::new("reg")
            .args(["query", &key, "/v", "pv"])
            .output()
        else {
            continue;
        };
        if out.status.success() {
            if let Some(version) = parse_reg_value(&String::from_utf8_lossy(&out.stdout)) {
                return Some(version);
            }
        }
    }
    None
}

/// Reads the data of a `REG_SZ` line from `reg query` output.
fn parse_reg_value(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains("REG_SZ"))
        .and_then(|line| line.split_whitespace().last())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod doctor_tests {
    use super::*;

    #[test]
    fn reg_output_yields_the_version() {
        let sample =
            "\r\nHKEY_CURRENT_USER\\Software\\...\\{F30}\r\n    pv    REG_SZ    155.0.4283.62\r\n";
        assert_eq!(parse_reg_value(sample).as_deref(), Some("155.0.4283.62"));
    }

    #[test]
    fn missing_key_output_yields_nothing() {
        assert_eq!(
            parse_reg_value(
                "ERROR: The system was unable to find the specified registry key or value."
            ),
            None
        );
    }

    #[test]
    fn doctor_runs_without_panicking() {
        // The exit code depends on the machine; the point is that every check completes.
        let _ = doctor();
    }
}

/// Writes the SBOMs and the attestation for the application named `tpt-shell`, reading the
/// lock file in the current directory. Exits 1 when the licence gate fails, after writing.
fn compliance(out: &Path) -> Result<ExitCode, String> {
    use tpt_shell_compliance as c;

    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed; is Cargo.lock up to date?".into());
    }
    let metadata = String::from_utf8_lossy(&output.stdout).to_string();
    let app = c::app_from_metadata(&metadata, "tpt-shell")?;
    let components = c::components_from_metadata(&metadata)?;
    let lock =
        fs::read(Path::new("Cargo.lock")).map_err(|e| format!("cannot read Cargo.lock: {e}"))?;

    let namespace = format!(
        "https://tpt-solutions.example/spdx/{}-{}",
        app.name, app.version
    );
    // SOURCE_DATE_EPOCH (reproducible builds) fixes the time; otherwise the current time is used.
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs())
        });
    let created = c::iso8601_utc(epoch);
    let cdx = serde_json::to_string_pretty(&c::cyclonedx(&components, &app, Some(&created)))
        .map_err(|e| e.to_string())?
        + "\n";
    let spdx =
        serde_json::to_string_pretty(&c::spdx(&components, &app, &namespace, Some(&created)))
            .map_err(|e| e.to_string())?
            + "\n";
    let violations = c::license_violations(&components);
    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let rustc = tool_version("rustc", &["--version"]).unwrap_or_else(|| "unknown".into());
    let attestation = c::attestation(
        &app,
        &rustc,
        &c::digest(&lock),
        &[
            ("sbom.cdx.json", c::digest(cdx.as_bytes())),
            ("sbom.spdx.json", c::digest(spdx.as_bytes())),
        ],
        &violations,
        git_commit.as_deref(),
        &c::Reproducibility {
            source_date_epoch: std::env::var("SOURCE_DATE_EPOCH")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok()),
            locked: true,
            cargo_flags: vec!["--locked".into()],
        },
    );
    let attestation_text =
        serde_json::to_string_pretty(&attestation).map_err(|e| e.to_string())? + "\n";

    fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    write(&out.join("sbom.cdx.json"), &cdx)?;
    write(&out.join("sbom.spdx.json"), &spdx)?;
    write(&out.join("attestation.json"), &attestation_text)?;
    println!("wrote {} components to {}", components.len(), out.display());

    if violations.is_empty() {
        println!("licence gate: pass");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("licence gate: fail");
        for v in &violations {
            println!("  {v}");
        }
        Ok(ExitCode::from(1))
    }
}

/// Page for `new typescript`. It loads the compiled script and the page's own styles.
const TS_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>My App</title>
  <style>
    :root { color-scheme: light dark; }
    body { font-family: system-ui, sans-serif; margin: 2rem; }
  </style>
</head>
<body>
  <h1>My App</h1>
  <p id="out">Starting…</p>
  <script type="module" src="dist/main.js"></script>
</body>
</html>
"#;

/// Starter TypeScript. It calls the sample command through the generated client.
const TS_MAIN: &str = r#"import { commands } from "./commands.js";

const out = document.getElementById("out");

commands
  .greet({ name: "world" })
  .then((reply) => {
    if (out) out.textContent = String(reply);
  })
  .catch((error: { code: string; message: string }) => {
    if (out) out.textContent = `${error.code}: ${error.message}`;
  });
"#;

/// `tsconfig.json` for the starter. Strict; output goes to `dist/`, which `index.html` loads.
const TS_CONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ES2020",
    "lib": ["ES2020", "DOM"],
    "strict": true,
    "noEmitOnError": true,
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src"]
}
"#;

/// The sample command behind the starter client: `greet(name)` returns a greeting.
fn sample_contract() -> tpt_shell_ipc::contract::Contract {
    use tpt_shell_ipc::contract::{Command, Contract, Kind};
    use tpt_shell_ipc::schema::{Field, Schema};
    const FIELDS: &[Field] = &[Field {
        name: "name",
        schema: Schema::Str {
            max_len: 64,
            chars: None,
        },
    }];
    let mut contract = Contract::new();
    contract
        .add(Command {
            name: "greet",
            kind: Kind::Sync,
            capability: None,
            args: Schema::Struct(FIELDS),
        })
        .expect("the sample contract has one command");
    contract
}

fn new_typescript(dir: &Path) -> Result<(), String> {
    prepare_dir(dir)?;
    write(&dir.join("tpt-shell.toml"), VANILLA_MANIFEST)?;
    write(&dir.join("index.html"), TS_PAGE)?;
    write(&dir.join("tsconfig.json"), TS_CONFIG)?;
    fs::create_dir_all(dir.join("src")).map_err(|e| format!("cannot create src: {e}"))?;
    write(&dir.join("src").join("main.ts"), TS_MAIN)?;
    write(&dir.join("src").join("tpt-shell.d.ts"), TS_RUNTIME_DECL)?;
    write(
        &dir.join("src").join("commands.ts"),
        &tpt_shell_ipc::typescript::generate(&sample_contract()),
    )?;
    println!(
        "created {} (build with: npx -p typescript tsc -p .)",
        dir.display()
    );
    Ok(())
}

/// Creates `dir` if needed. Refuses a directory that already has files in it.
fn prepare_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        let mut entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if entries.next().is_some() {
            return Err(format!(
                "{} is not empty; choose a new directory",
                dir.display()
            ));
        }
        Ok(())
    } else {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))
    }
}

/// Declares the runtime object that `js/runtime/tpt-shell.js` adds before the page runs.
const TS_RUNTIME_DECL: &str = r#"// The runtime object that tpt-shell adds at document start (js/runtime/tpt-shell.js).
interface Window {
  tpt: {
    invoke(cmd: string, args?: unknown): Promise<unknown>;
    on(event: string, handler: (data: unknown) => void): () => void;
    readonly caps: { readonly engine: "webview2"; readonly version: string | null };
  };
}
"#;

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn typescript_template_writes_a_client_for_its_sample_command() {
        let dir = std::env::temp_dir().join(format!("tpt-cli-ts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        new_typescript(&dir).unwrap();
        let client = fs::read_to_string(dir.join("src").join("commands.ts")).unwrap();
        assert!(client.contains("greet(args: GreetArgs): Promise<unknown>"));
        assert!(dir.join("tsconfig.json").exists());
        let decl = fs::read_to_string(dir.join("src").join("tpt-shell.d.ts")).unwrap();
        assert!(decl.contains("on(event: string"));
        assert!(decl.contains("readonly caps:"));
        assert!(
            new_typescript(&dir).is_err(),
            "a second run must not overwrite"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, windows))]
mod startup_tests {
    use super::*;

    #[test]
    fn install_and_uninstall_a_sign_in_entry() {
        let exe = std::env::current_exe().expect("current exe");
        let name = format!("tpt-cli-test-{}", std::process::id());
        assert!(run(&["install-service", &name, exe.to_str().unwrap()]).is_ok());
        let command = tpt_shell_platform_windows::startup::command(&name).expect("entry");
        assert!(
            command.starts_with('"') && command.ends_with('"'),
            "{command}"
        );
        assert!(run(&["uninstall-service", &name]).is_ok());
        assert_eq!(tpt_shell_platform_windows::startup::command(&name), None);
    }

    #[test]
    fn install_refuses_a_program_that_does_not_exist() {
        assert!(run(&["install-service", "tpt-missing", r"C:\no\such\app.exe"]).is_err());
    }
}
