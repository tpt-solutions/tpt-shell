// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell`: explain error codes, check manifests, and create starter projects.
//!
//! Exit codes: 0 success, 1 the checked input has problems, 2 bad usage or I/O failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt_shell_audit::AuditError;

mod add;
mod audit_explain;
mod dev;
mod kiosk;
mod licences;
mod packaging;
mod permissions;
mod replay_cmd;
// Windows uses its own sign-in entry, so the macOS and Linux writers are unused there.
#[cfg_attr(windows, allow(dead_code))]
mod service;
mod visual_cmd;
use tpt_shell_errors::lookup;

const USAGE: &str = "usage:
  tpt-shell explain <TS####>
  tpt-shell dev <directory> [--port <port>]   serve files on 127.0.0.1, reload pages on change
  tpt-shell build [directory]                 release build (cargo build --release --locked)
  tpt-shell add <kiosk|devices|audit|agent> [tpt-shell.toml]   add a manifest section
  tpt-shell manifest check <tpt-shell.toml>
  tpt-shell contract diff <old-contract.json> <new-contract.json>
  tpt-shell contract check <new-contract.json> --baseline <old-contract.json>
  tpt-shell contract gen <contract.json> <out-directory>   write the TypeScript client (commands.ts)
  tpt-shell permissions suggest <contract.json>   the capability grants the commands need
  tpt-shell audit explain <audit-log>             plain-language summary of a verified audit log
  tpt-shell new vanilla <directory>
  tpt-shell new typescript <directory>
  tpt-shell new rust <directory> [--tpt-shell <path to crates/tpt-shell>]
  tpt-shell new --template <pos|kiosk|audit-app|agent-demo|appfront-dom> <directory> [--tpt-shell <path>]
  tpt-shell run [directory]                   build and launch the Rust project (cargo run)
  tpt-shell init [directory] [--tpt-shell <path to crates/tpt-shell>]   add tpt-shell to an existing crate
  tpt-shell doctor
  tpt-shell compliance <output-directory> [--key <signing-key-file>]
  tpt-shell audit verify <audit.jsonl> [--key <public-key-file>] [--anchors <anchor-file>]
  tpt-shell audit verify <directory> --rotated <stem> [--key <public-key-file>] [--anchors <anchor-file>]
  tpt-shell audit export <audit.jsonl> --format csv|jsonl
  tpt-shell audit view <audit.jsonl>
  tpt-shell replay <file.tptreplay> [--strict | --lenient]
  tpt-shell report <out.json> [--replay <file>] [--log <file>]... [--audit <audit.jsonl>]
                              [--version <name>=<value>]... [--redact <text>]...
  tpt-shell visual compare <baseline-dir> <dir> [dir...] [--out <report.html>]
                              [--tolerance <0..1>] [--max-changed <0..1>]
  tpt-shell runtime pin <tpt-shell.toml> (--cab <file> | --url <https-url>) --out <dir>
  tpt-shell runtime list <dir>...
  tpt-shell vendor [directory]
  tpt-shell docs --offline [output.zip]
  tpt-shell kiosk sign <config.json> --key <key-file>
  tpt-shell install-service <name> <program>     start the program at sign-in
  tpt-shell uninstall-service <name>             remove that sign-in entry";

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
    let reloads = std::sync::Arc::new(dev::Reloads::default());
    dev::watch(
        root.clone(),
        std::time::Duration::from_millis(500),
        std::sync::Arc::clone(&reloads),
    );
    dev::serve(&root, addr, &reloads, |bound| {
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
        ["contract", "gen", json, out] => contract_gen(Path::new(json), Path::new(out)),
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
        ["new", "rust", dir] => new_rust(Path::new(dir), None).map(|()| ExitCode::SUCCESS),
        ["new", "rust", dir, "--tpt-shell", checkout] => {
            new_rust(Path::new(dir), Some(Path::new(checkout))).map(|()| ExitCode::SUCCESS)
        }
        ["new", "--template", name, dir] => {
            new_from_template(name, Path::new(dir), None).map(|()| ExitCode::SUCCESS)
        }
        ["new", "--template", name, dir, "--tpt-shell", checkout] => {
            new_from_template(name, Path::new(dir), Some(Path::new(checkout)))
                .map(|()| ExitCode::SUCCESS)
        }
        ["init"] => init(Path::new("."), None).map(|()| ExitCode::SUCCESS),
        ["init", dir] => init(Path::new(dir), None).map(|()| ExitCode::SUCCESS),
        ["init", dir, "--tpt-shell", checkout] => {
            init(Path::new(dir), Some(Path::new(checkout))).map(|()| ExitCode::SUCCESS)
        }
        ["run"] => run_project(Path::new(".")),
        ["run", dir] => run_project(Path::new(dir)),
        ["doctor"] => Ok(doctor()),
        ["build"] => build(Path::new(".")),
        ["build", dir] => build(Path::new(dir)),
        ["add", feature] => add::add(feature, Path::new("tpt-shell.toml")),
        ["add", feature, manifest] => add::add(feature, Path::new(manifest)),
        ["compliance", dir] => compliance(Path::new(dir), None),
        ["compliance", dir, "--key", key] => compliance(Path::new(dir), Some(Path::new(key))),
        ["audit", "verify", rest @ ..] => audit_verify_args(rest),
        ["audit", "export", path, "--format", "csv"] => audit_export_csv(Path::new(path)),
        ["audit", "export", path, "--format", "jsonl"] => audit_export_jsonl(Path::new(path)),
        ["audit", "view", path] => audit_view(Path::new(path)),
        ["audit", "explain", path] => audit_explain_file(Path::new(path)),
        ["permissions", "suggest", json] => permissions::suggest(Path::new(json)),
        ["replay", rest @ ..] => replay_cmd::replay(rest),
        ["report", rest @ ..] => replay_cmd::report(rest),
        ["visual", "compare", rest @ ..] => visual_cmd::compare(rest),
        ["runtime", rest @ ..] => packaging::runtime(rest),
        ["vendor", rest @ ..] => packaging::vendor(rest),
        ["docs", rest @ ..] => packaging::docs(rest),
        ["kiosk", "sign", config, "--key", key] => kiosk::sign(Path::new(config), Path::new(key)),
        ["install-service", name, program] => install_service(name, Path::new(program)),
        ["uninstall-service", name] => uninstall_service(name),
        _ => Err(USAGE.to_string()),
    }
}

/// Writes the TypeScript client for the contract in `json` into `out`: `commands.ts`, always,
/// and `tpt-shell.d.ts` when the folder does not have one. The JSON comes from an app built on
/// tpt-shell, by running it with `--tpt-write-contract <file>`.
fn contract_gen(json: &Path, out: &Path) -> Result<ExitCode, String> {
    let text =
        fs::read_to_string(json).map_err(|e| format!("cannot read {}: {e}", json.display()))?;
    let contract = tpt_shell_ipc::contract_load::from_json(&text)?;
    fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let client = out.join("commands.ts");
    write(&client, &tpt_shell_ipc::typescript::generate(&contract))?;
    let decl = out.join("tpt-shell.d.ts");
    if !decl.exists() {
        write(&decl, TS_RUNTIME_DECL)?;
    }
    println!("wrote {}", client.display());
    Ok(ExitCode::SUCCESS)
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

/// macOS and Linux: a LaunchAgent or a systemd user unit under the user's home folder. The
/// entry is written, not started; the output says how to start it.
#[cfg(not(windows))]
fn startup_entry(name: &str, program: Option<&Path>) -> Result<ExitCode, String> {
    let platform = service::platform()
        .ok_or("sign-in entries are supported on Windows, macOS and Linux only")?;
    let home = service::home_dir()?;
    match program {
        Some(program) => {
            let spec = tpt_shell_supervisor::install::ServiceSpec::new(name, program, Vec::new())
                .map_err(|e| e.to_string())?;
            let path = service::install(platform, &home, &spec)?;
            println!("{name} will start at sign-in: {}", path.display());
            println!("{}", service::start_hint(platform, &path));
        }
        None => match service::uninstall(platform, &home, name)? {
            Some(path) => println!("removed {}", path.display()),
            None => println!("{name} has no sign-in entry; nothing to remove"),
        },
    }
    Ok(ExitCode::SUCCESS)
}

/// A broken chain, an anchor or segment mismatch, or a bad signature is a problem in the checked
/// input (exit 1). Anything else is an I/O failure (exit 2), reported through the `Err` path.
fn audit_failure(error: AuditError) -> Result<ExitCode, String> {
    use tpt_shell_audit::anchor::CODE_ANCHOR_MISMATCH;
    use tpt_shell_audit::keys::CODE_SIGNATURE_INVALID;
    use tpt_shell_audit::segments::CODE_SEGMENT;
    let is_problem = match &error {
        AuditError::Chain { .. } => true,
        AuditError::Refused { code, .. } => {
            matches!(
                *code,
                CODE_ANCHOR_MISMATCH | CODE_SIGNATURE_INVALID | CODE_SEGMENT
            )
        }
        AuditError::Io(_) => false,
    };
    if is_problem {
        eprintln!("{error}");
        return Ok(ExitCode::from(1));
    }
    Err(error.to_string())
}

/// The options of `audit verify`, after the file or folder.
#[derive(Debug, Default, PartialEq)]
struct VerifySources<'a> {
    key: Option<&'a Path>,
    anchors: Option<&'a Path>,
    rotated: Option<&'a str>,
}

/// Parses `audit verify <path> [--key f] [--anchors f] [--rotated stem]`.
fn audit_verify_args(args: &[&str]) -> Result<ExitCode, String> {
    let mut path: Option<&str> = None;
    let mut sources = VerifySources::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--key" | "--anchors" | "--rotated" => {
                let Some(&value) = rest.next() else {
                    return Err(format!("{arg} needs a value\n{USAGE}"));
                };
                match *arg {
                    "--key" => sources.key = Some(Path::new(value)),
                    "--anchors" => sources.anchors = Some(Path::new(value)),
                    _ => sources.rotated = Some(value),
                }
            }
            value if path.is_none() && !value.starts_with("--") => path = Some(value),
            _ => return Err(USAGE.to_string()),
        }
    }
    let path = path.ok_or_else(|| USAGE.to_string())?;
    audit_verify(Path::new(path), &sources)
}

/// Checks the hash chain, the checkpoint signatures when a public key is given, and the anchors
/// when an anchor file is given. With `--rotated`, the path is the folder of the segments.
fn audit_verify(path: &Path, sources: &VerifySources) -> Result<ExitCode, String> {
    use tpt_shell_audit as a;

    let key = match sources.key {
        None => None,
        Some(key) => {
            let text = fs::read_to_string(key)
                .map_err(|e| format!("cannot read {}: {e}", key.display()))?;
            Some(a::verifying_key_from_hex(&text).ok_or_else(|| {
                format!("{} is not a 64-digit hex Ed25519 public key", key.display())
            })?)
        }
    };
    let anchors = match sources.anchors {
        None => None,
        Some(file) => match a::anchor::read_file_anchors(file) {
            Ok(points) => Some(points),
            Err(AuditError::Io(reason)) => {
                return Err(format!("cannot read {}: {reason}", file.display()))
            }
            Err(error) => return audit_failure(error),
        },
    };
    let key_ref = key.as_ref();
    let result = match (sources.rotated, anchors.as_deref()) {
        (Some(stem), points) => a::verify_rotated(path, stem, key_ref, points.unwrap_or(&[])),
        (None, Some(points)) => a::verify_file_anchored(path, key_ref, points),
        (None, None) => match key_ref {
            None => a::verify_file(path),
            Some(key) => a::verify_file_with_key(path, key),
        },
    };
    match result {
        Ok(summary) => {
            println!(
                "ok: {} records, {} checkpoints, last hash {}",
                summary.records, summary.checkpoints, summary.last_hash
            );
            if let Some(points) = &anchors {
                println!("anchors: {} checked", points.len());
            }
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

/// Verifies the audit log, then prints its plain-language summary.
fn audit_explain_file(path: &Path) -> Result<ExitCode, String> {
    if let Err(error) = tpt_shell_audit::verify_file(path) {
        return audit_failure(error);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    for line in audit_explain::explain(&text)? {
        println!("{line}");
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

/// The starter manifest for `new vanilla` (templates/vanilla/tpt-shell.toml).
const VANILLA_MANIFEST: &str = include_str!("../../../templates/vanilla/tpt-shell.toml");

/// The starter page for `new vanilla` (templates/vanilla/index.html). It has no external scripts
/// or styles.
const VANILLA_PAGE: &str = include_str!("../../../templates/vanilla/index.html");

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
        assert!(run(&["audit", "verify", "x.jsonl", "--anchors"]).is_err());
        assert!(run(&["audit", "verify", "x.jsonl", "--rotated"]).is_err());
        assert!(run(&["audit", "verify", "x.jsonl", "--bogus"]).is_err());
        assert!(run(&["audit", "verify", "--key", "k"]).is_err(), "no path");
    }

    #[test]
    fn an_empty_anchor_file_verifies_a_good_log() {
        let path = sample_audit_log("anchored");
        let anchors = scratch("anchors-empty");
        fs::write(&anchors, "").unwrap();
        let code = run(&[
            "audit",
            "verify",
            path.to_str().unwrap(),
            "--anchors",
            anchors.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&anchors);
    }

    #[test]
    fn an_anchor_that_the_log_does_not_hold_is_a_problem() {
        let path = sample_audit_log("anchor-missing");
        let anchors = scratch("anchors-bad");
        let zeros = "0".repeat(64);
        fs::write(&anchors, format!("0 1700000000000 {zeros}\n")).unwrap();
        let code = run(&[
            "audit",
            "verify",
            path.to_str().unwrap(),
            "--anchors",
            anchors.to_str().unwrap(),
        ])
        .unwrap();
        assert_eq!(code, ExitCode::from(1));
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&anchors);
    }

    #[test]
    fn a_folder_with_no_segments_is_a_problem() {
        let dir = scratch("no-segments");
        fs::create_dir_all(&dir).unwrap();
        let code = run(&[
            "audit",
            "verify",
            dir.to_str().unwrap(),
            "--rotated",
            "audit",
        ])
        .unwrap();
        assert_eq!(code, ExitCode::from(1));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compliance_refuses_without_a_readable_deny_toml() {
        // Tests run in the crate folder, which has no deny.toml of its own.
        let dir = scratch("compliance-policy");
        let error = compliance(&dir, None).unwrap_err();
        assert!(error.contains("TS0120"), "{error}");
        assert!(
            !dir.exists(),
            "nothing is written when the policy is unreadable"
        );
    }

    #[test]
    fn compliance_takes_its_key_only_as_a_flag() {
        assert!(run(&["compliance", "out", "--key"]).is_err());
        assert!(run(&["compliance", "out", "--sign"]).is_err());
        assert!(run(&["compliance"]).is_err());
    }

    #[test]
    fn kiosk_sign_and_the_other_new_commands_refuse_bad_usage() {
        assert!(run(&["kiosk", "sign", "kiosk.json"]).is_err());
        assert!(run(&["kiosk", "sign", "kiosk.json", "--key"]).is_err());
        assert!(run(&["replay"]).is_err());
        assert!(run(&["report"]).is_err());
        assert!(run(&["visual", "compare", "one"]).is_err());
        assert!(run(&["runtime"]).is_err());
        assert!(run(&["vendor", "a", "b"]).is_err());
        assert!(run(&["docs"]).is_err());
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
    match licence_check() {
        None => println!("licence policy: deny.toml not found here (run from the workspace root)"),
        Some(Err(e)) => {
            ok = false;
            println!("licence policy: cannot check: {e}");
        }
        Some(Ok(problems)) if problems.is_empty() => println!("licence policy: pass"),
        Some(Ok(problems)) => {
            ok = false;
            println!("licence policy: fail");
            for problem in &problems {
                println!("  {problem}");
            }
        }
    }
    // Linux: which GTK and WebKit adapter would run. No adapter is a failure, because the shell
    // cannot open a window. Opens the libraries only; it does not start GTK or need a display.
    #[cfg(target_os = "linux")]
    {
        let probe = tpt_shell_platform_linux::probe::probe();
        if !probe.ok() {
            ok = false;
        }
        for line in probe.report() {
            println!("{line}");
        }
    }
    // The project in the current folder, when it has a manifest: a bad manifest is a failure.
    if let Some((lines, failed)) = project_check(Path::new(".")) {
        if failed {
            ok = false;
        }
        for line in lines {
            println!("{line}");
        }
        #[cfg(windows)]
        for line in loader_lines(Path::new(".")) {
            println!("{line}");
        }
    }
    // Optional packaging tools and platform minimums: warnings only, never a failure.
    for line in packaging::doctor_lines() {
        println!("{line}");
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Checks the project's `tpt-shell.toml` in `dir`, if it has one. Returns the lines to print, and
/// whether the manifest is a problem. Each problem carries its fix hint from the error registry.
fn project_check(dir: &Path) -> Option<(Vec<String>, bool)> {
    let path = dir.join("tpt-shell.toml");
    if !path.is_file() {
        return None;
    }
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            return Some((
                vec![format!(
                    "project manifest: cannot read {}: {e}",
                    path.display()
                )],
                true,
            ))
        }
    };
    match tpt_shell_manifest::parse(&text) {
        Ok(_) => Some((vec!["project manifest: ok".to_string()], false)),
        Err(e) => {
            let mut lines = vec![format!("project manifest: {e}")];
            if let Some(entry) = lookup(e.code) {
                lines.push(format!("  {}", entry.fix));
            }
            Some((lines, true))
        }
    }
}

/// Where the WebView2 loader is, for a project. Missing is a warning: the window cannot open
/// without it, but `doctor` does not fail for it, since the loader is often copied in at run time.
#[cfg(windows)]
fn loader_lines(dir: &Path) -> Vec<String> {
    let found = ["", "target/debug", "target/release"]
        .iter()
        .map(|sub| dir.join(sub).join("WebView2Loader.dll"))
        .find(|path| path.is_file());
    match found {
        Some(path) => vec![format!("WebView2 loader: {}", path.display())],
        None => vec![
            "WebView2 loader: not beside a built program. Copy WebView2Loader.dll from the WebView2 SDK (build/native/x64/) into target/debug/".to_string(),
        ],
    }
}

/// Checks the dependency licences against the allow list in `deny.toml` in the current
/// directory. `None` when there is no `deny.toml` here. Read-only.
fn licence_check() -> Option<Result<Vec<String>, String>> {
    let text = fs::read_to_string("deny.toml").ok()?;
    Some(check_licences(&text))
}

fn check_licences(deny: &str) -> Result<Vec<String>, String> {
    let allowed = licences::allow_list(deny)?;
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .map_err(|e| format!("cannot run cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed; is Cargo.lock up to date?".into());
    }
    let metadata = String::from_utf8_lossy(&output.stdout).to_string();
    let components = tpt_shell_compliance::components_from_metadata(&metadata)?;
    Ok(licences::violations(&components, &allowed))
}

/// Runs `cargo build --release --locked` in `dir`, which must hold a `Cargo.toml`. Compliance
/// outputs (SBOM, attestation) come from `tpt-shell compliance`, added in M10.
fn build(dir: &Path) -> Result<ExitCode, String> {
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "{} has no Cargo.toml; run `tpt-shell build` in the app's Rust project",
            dir.display()
        ));
    }
    let status = std::process::Command::new("cargo")
        .args(["build", "--release", "--locked"])
        .current_dir(dir)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() {
        println!("built release in {}", dir.display());
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
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

/// Writes the SBOMs, the attestation, `licenses.html` and `THIRD_PARTY_NOTICES` for the application
/// named `tpt-shell`. The inputs are the lock file and `deny.toml` in the current directory, and
/// cargo metadata. With `key` (a file of 64 hex digits, the audit key format), every output also
/// gets a detached `.sig`. Exits 1 when the licence gate refuses, after writing.
fn compliance(out: &Path, key: Option<&Path>) -> Result<ExitCode, String> {
    use tpt_shell_audit::keys::KeySupplier;
    use tpt_shell_compliance as c;

    // The allow list comes from deny.toml, so the gate and cargo-deny agree. Without it there is
    // nothing to judge the licences by, so it is checked first.
    let deny = fs::read_to_string("deny.toml")
        .map_err(|e| format!("{}: cannot read deny.toml: {e}", c::CODE_POLICY))?;
    let allow = c::allow_list_from_deny_toml(&deny)?;
    let signer = match key {
        None => None,
        Some(key) => Some(
            tpt_shell_audit::keys::FileKeySupplier::new(key)
                .signer()
                .map_err(|e| e.to_string())?,
        ),
    };

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
    let lock_text =
        fs::read_to_string("Cargo.lock").map_err(|e| format!("cannot read Cargo.lock: {e}"))?;
    let locked = c::lock::sourced_packages(&lock_text)?;
    let violations = c::gate(&components, &locked, &allow);

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
    let notices = c::notices::notices_from_metadata(&metadata)?;
    let licenses = c::notices::licenses_html(&app, &notices);
    let notice_text = c::notices::third_party_notices(&app, &notices);
    let git_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let rustc = tool_version("rustc", &["--version"]).unwrap_or_else(|| "unknown".into());
    let attestation = c::attestation_with_allow(
        &app,
        &rustc,
        &c::digest(lock_text.as_bytes()),
        &[
            ("sbom.cdx.json", c::digest(cdx.as_bytes())),
            ("sbom.spdx.json", c::digest(spdx.as_bytes())),
            ("licenses.html", c::digest(licenses.as_bytes())),
            ("THIRD_PARTY_NOTICES", c::digest(notice_text.as_bytes())),
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
        &allow,
    );
    let attestation_text =
        serde_json::to_string_pretty(&attestation).map_err(|e| e.to_string())? + "\n";

    let files = [
        ("sbom.cdx.json", cdx),
        ("sbom.spdx.json", spdx),
        ("licenses.html", licenses),
        ("THIRD_PARTY_NOTICES", notice_text),
        ("attestation.json", attestation_text),
    ];
    fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    for (name, text) in &files {
        write(&out.join(name), text)?;
    }
    println!("wrote {} components to {}", components.len(), out.display());

    if let (Some(signer), Some(key)) = (&signer, key) {
        for (name, text) in &files {
            let signature = c::sign_artifact(signer.as_ref(), text.as_bytes())?;
            write(&out.join(format!("{name}.sig")), &(signature + "\n"))?;
        }
        println!("signed {} files with {}", files.len(), key.display());
    }

    if violations.is_empty() {
        println!("licence gate: pass");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("licence gate: fail");
        eprintln!("{}", c::gate_failure(&violations));
        Ok(ExitCode::from(1))
    }
}

/// Page for `new typescript` (templates/typescript/index.html).
const TS_PAGE: &str = include_str!("../../../templates/typescript/index.html");

/// Manifest for `new typescript` (templates/typescript/tpt-shell.toml).
const TS_MANIFEST: &str = include_str!("../../../templates/typescript/tpt-shell.toml");

/// Starter TypeScript (templates/typescript/src/main.ts). It calls the sample command through
/// the generated client.
const TS_MAIN: &str = include_str!("../../../templates/typescript/src/main.ts");

/// `tsconfig.json` for the starter (templates/typescript/tsconfig.json).
const TS_CONFIG: &str = include_str!("../../../templates/typescript/tsconfig.json");

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
    write(&dir.join("tpt-shell.toml"), TS_MANIFEST)?;
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

/// The Cargo manifest for `new rust` (templates/rust/Cargo.toml.tmpl).
const RUST_CARGO: &str = include_str!("../../../templates/rust/Cargo.toml.tmpl");

/// The entry point for `new rust` (templates/rust/src/main.rs.tmpl).
const RUST_MAIN: &str = include_str!("../../../templates/rust/src/main.rs.tmpl");

/// The page for `new rust` (templates/rust/index.html.tmpl).
const RUST_PAGE: &str = include_str!("../../../templates/rust/index.html.tmpl");

/// The manifest for `new rust` (templates/rust/tpt-shell.toml.tmpl).
const RUST_MANIFEST: &str = include_str!("../../../templates/rust/tpt-shell.toml.tmpl");

/// Writes a Cargo project that opens one window. The project depends on the tpt-shell crate by
/// path, because the crates are not published. `checkout` is the `crates/tpt-shell` folder; by
/// default it is the one this CLI was built from.
fn new_rust(dir: &Path, checkout: Option<&Path>) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no usable name", dir.display()))?;
    if !valid_crate_name(name) {
        return Err(format!(
            "{name:?} is not a valid crate name: use lowercase letters, digits, `-` or `_`, starting with a letter"
        ));
    }
    let checkout = crate_dir(checkout)?;
    // Forward slashes need no escaping inside a TOML string.
    let checkout = without_verbatim_prefix(&checkout.display().to_string()).replace('\\', "/");
    let render = |template: &str| {
        template
            .replace("{{name}}", name)
            .replace("{{tpt_shell_path}}", &checkout)
    };
    prepare_dir(dir)?;
    fs::create_dir_all(dir.join("src")).map_err(|e| format!("cannot create src: {e}"))?;
    write(&dir.join("Cargo.toml"), &render(RUST_CARGO))?;
    write(&dir.join("src").join("main.rs"), &render(RUST_MAIN))?;
    write(&dir.join("index.html"), &render(RUST_PAGE))?;
    write(&dir.join("tpt-shell.toml"), &render(RUST_MANIFEST))?;
    println!(
        "created {} (run with: cargo run, from inside it)",
        dir.display()
    );
    Ok(())
}

/// The Rust starter projects in templates/, by the name `new --template` takes.
const RUST_TEMPLATES: &[&str] = &["pos", "kiosk", "audit-app", "agent-demo", "appfront-dom"];

/// Writes a copy of one starter project from templates/. Its path dependencies point at the
/// checkout's crates, so the copy builds on its own.
fn new_from_template(name: &str, dir: &Path, checkout: Option<&Path>) -> Result<(), String> {
    if !RUST_TEMPLATES.contains(&name) {
        return Err(format!(
            "unknown template {name:?}; choose one of: {}",
            RUST_TEMPLATES.join(", ")
        ));
    }
    let crate_dir = crate_dir(checkout)?;
    // crates/tpt-shell sits two folders below the repository root, which holds templates/.
    let repo = crate_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| format!("{} is not inside a repository", crate_dir.display()))?;
    let source = repo.join("templates").join(name);
    if !source.is_dir() {
        return Err(format!("no template folder at {}", source.display()));
    }
    // Forward slashes need no escaping inside a TOML string.
    let crates =
        without_verbatim_prefix(&repo.join("crates").display().to_string()).replace('\\', "/");
    prepare_dir(dir)?;
    copy_template(&source, dir, &crates)?;
    println!("created {} from the {name} template", dir.display());
    Ok(())
}

/// Copies a template folder. Cargo.toml paths into the checkout's crates folder are rewritten,
/// and the copy becomes its own workspace.
fn copy_template(from: &Path, to: &Path, crates: &str) -> Result<(), String> {
    let entries = fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
            copy_template(&source, &target, crates)?;
        } else if entry.file_name() == "Cargo.toml" {
            let text = fs::read_to_string(&source)
                .map_err(|e| format!("cannot read {}: {e}", source.display()))?
                .replace("\"../../crates/", &format!("\"{crates}/"));
            let text = if text.contains("[workspace]") {
                text
            } else {
                format!("{text}\n[workspace]\n")
            };
            write(&target, &text)?;
        } else {
            fs::copy(&source, &target)
                .map_err(|e| format!("cannot copy {}: {e}", source.display()))?;
        }
    }
    Ok(())
}

/// The `crates/tpt-shell` folder: the one given, or the one this CLI was built from. Refused
/// unless it has a Cargo.toml. Returned canonical.
fn crate_dir(checkout: Option<&Path>) -> Result<PathBuf, String> {
    let checkout = match checkout {
        Some(path) => path.to_path_buf(),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tpt-shell"),
    };
    if !checkout.join("Cargo.toml").is_file() {
        return Err(format!(
            "no tpt-shell crate at {} (pass --tpt-shell <path to crates/tpt-shell>)",
            checkout.display()
        ));
    }
    fs::canonicalize(&checkout).map_err(|e| format!("cannot resolve {}: {e}", checkout.display()))
}

/// Adds tpt-shell to the existing Rust project in `dir`: a path dependency in Cargo.toml, and a
/// `tpt-shell.toml` if there is none. The project's own files are left alone, so the app's
/// `main.rs` is for the user to change. Running it twice adds nothing the second time.
fn init(dir: &Path, checkout: Option<&Path>) -> Result<(), String> {
    let cargo_path = dir.join("Cargo.toml");
    let text = fs::read_to_string(&cargo_path).map_err(|_| {
        format!(
            "{} has no Cargo.toml; `tpt-shell init` adds tpt-shell to an existing Rust project",
            dir.display()
        )
    })?;
    let name = package_name(&text).ok_or_else(|| {
        format!(
            "{} has no [package] name; is it a Rust project?",
            cargo_path.display()
        )
    })?;
    if has_dependency(&text, "tpt-shell") {
        println!("{} already depends on tpt-shell", cargo_path.display());
    } else {
        let checkout = crate_dir(checkout)?;
        // Forward slashes need no escaping inside a TOML string.
        let checkout = without_verbatim_prefix(&checkout.display().to_string()).replace('\\', "/");
        let updated =
            insert_dependency(&text, &format!("tpt-shell = {{ path = \"{checkout}\" }}\n"));
        write(&cargo_path, &updated)?;
        println!("added tpt-shell to {}", cargo_path.display());
    }
    let manifest = dir.join("tpt-shell.toml");
    if manifest.exists() {
        println!("kept {}", manifest.display());
    } else {
        write(&manifest, &RUST_MANIFEST.replace("{{name}}", &name))?;
        println!("created {}", manifest.display());
    }
    println!("next: open a window from main.rs, as in the rust starter (tpt-shell new rust)");
    Ok(())
}

/// The `name` in the `[package]` table of a Cargo.toml.
fn package_name(cargo: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line.strip_prefix("name") {
                let value = value.trim_start().strip_prefix('=')?.trim();
                return value
                    .strip_prefix('"')?
                    .strip_suffix('"')
                    .map(str::to_string);
            }
        }
    }
    None
}

/// Whether the Cargo.toml has a `name = ...` dependency line for `dep`.
fn has_dependency(cargo: &str, dep: &str) -> bool {
    cargo.lines().any(|line| {
        line.trim()
            .strip_prefix(dep)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// Adds `line` as the first entry of the `[dependencies]` table, or a new table at the end.
fn insert_dependency(cargo: &str, line: &str) -> String {
    let mut out = String::with_capacity(cargo.len() + line.len() + 32);
    let mut inserted = false;
    for chunk in cargo.split_inclusive('\n') {
        out.push_str(chunk);
        if !inserted && chunk.trim() == "[dependencies]" {
            out.push_str(line);
            inserted = true;
        }
    }
    if !inserted {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\n[dependencies]\n");
        out.push_str(line);
    }
    out
}

/// Builds and runs the project in `dir` with `cargo run`.
fn run_project(dir: &Path) -> Result<ExitCode, String> {
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "{} has no Cargo.toml; run `tpt-shell run` in the app's Rust project",
            dir.display()
        ));
    }
    let status = std::process::Command::new("cargo")
        .arg("run")
        .current_dir(dir)
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
}

/// A crate name that `cargo new` would accept: lowercase ASCII, starting with a letter.
fn valid_crate_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-' | '_'))
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

/// The runtime object that tpt-shell adds at document start (js/runtime/tpt-shell.js).
const TS_RUNTIME_DECL: &str = include_str!("../../../templates/typescript/src/tpt-shell.d.ts");

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

#[cfg(test)]
mod rust_template_tests {
    use super::*;

    #[test]
    fn rust_template_writes_a_project_that_depends_on_the_checkout() {
        let root = std::env::temp_dir().join(format!("tpt-cli-rust-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("my-app");
        new_rust(&dir, None).unwrap();
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-app\""), "{cargo}");
        assert!(cargo.contains("tpt-shell = { path = \""), "{cargo}");
        assert!(cargo.contains("[workspace]"), "{cargo}");
        assert!(
            !cargo.contains("{{"),
            "every placeholder is filled: {cargo}"
        );
        let main = fs::read_to_string(dir.join("src").join("main.rs")).unwrap();
        assert!(main.contains(".loader(loader_path())"), "{main}");
        assert!(main.contains(".name(\"my-app\")"), "{main}");
        assert!(dir.join("index.html").is_file());
        assert!(tpt_shell_manifest::parse(
            &fs::read_to_string(dir.join("tpt-shell.toml")).unwrap()
        )
        .is_ok());
        assert!(
            new_rust(&dir, None).is_err(),
            "a second run must not overwrite"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rust_template_refuses_a_bad_name_and_a_missing_checkout() {
        let root = std::env::temp_dir().join(format!("tpt-cli-rust-bad-{}", std::process::id()));
        assert!(new_rust(&root.join("My App"), None).is_err());
        assert!(new_rust(&root.join("9lives"), None).is_err());
        assert!(
            !root.join("My App").exists(),
            "nothing is written for a bad name"
        );
        let missing = Path::new(r"C:\no\such\tpt-shell");
        assert!(new_rust(&root.join("ok-name"), Some(missing)).is_err());
        assert!(!root.join("ok-name").exists());
    }

    #[test]
    fn a_template_copy_points_at_the_checkout_and_is_its_own_workspace() {
        let root = std::env::temp_dir().join(format!("tpt-cli-template-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("pos-copy");
        new_from_template("pos", &dir, None).unwrap();
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(!cargo.contains("../../crates"), "{cargo}");
        assert!(cargo.contains("[workspace]"), "{cargo}");
        assert!(dir.join("src").join("main.rs").is_file());
        assert!(dir.join("web").join("index.html").is_file());
        assert!(
            new_from_template("pos", &dir, None).is_err(),
            "a second run must not overwrite"
        );
        assert!(new_from_template("nope", &root.join("x"), None).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_refuses_a_folder_without_a_project() {
        let root = std::env::temp_dir().join(format!("tpt-cli-run-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        assert!(run_project(&root).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn init_adds_the_dependency_once_and_keeps_the_crate_files() {
        let dir = std::env::temp_dir().join(format!("tpt-cli-init-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        let original =
            "[package]\nname = \"legacy\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n";
        fs::write(dir.join("Cargo.toml"), original).unwrap();
        fs::write(dir.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        init(&dir, None).unwrap();
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("[dependencies]\ntpt-shell = { path = \""),
            "{cargo}"
        );
        assert!(
            cargo.contains("serde = \"1\""),
            "the other dependencies stay: {cargo}"
        );
        let manifest = fs::read_to_string(dir.join("tpt-shell.toml")).unwrap();
        assert!(manifest.contains("name = \"legacy\""), "{manifest}");
        assert_eq!(
            fs::read_to_string(dir.join("src").join("main.rs")).unwrap(),
            "fn main() {}\n"
        );
        init(&dir, None).unwrap();
        let again = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert_eq!(again.matches("tpt-shell = ").count(), 1, "{again}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_refuses_a_folder_without_a_cargo_project() {
        let dir = std::env::temp_dir().join(format!("tpt-cli-init-none-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(init(&dir, None).is_err());
        assert!(!dir.join("tpt-shell.toml").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dependency_is_found_by_its_exact_name() {
        assert!(has_dependency(
            "[dependencies]\ntpt-shell = \"0\"\n",
            "tpt-shell"
        ));
        assert!(!has_dependency(
            "[dependencies]\ntpt-shell-agent = \"0\"\n",
            "tpt-shell"
        ));
        assert_eq!(package_name("[dependencies]\nname = \"no\"\n"), None);
        assert_eq!(
            package_name("[package]\nname = \"yes\"\n"),
            Some("yes".to_string())
        );
    }

    #[test]
    fn a_broken_project_manifest_is_a_failure_with_a_fix_hint() {
        let dir = std::env::temp_dir().join(format!("tpt-cli-doctor-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(project_check(&dir).is_none(), "no manifest, no check");
        fs::write(dir.join("tpt-shell.toml"), "[app\n").unwrap();
        let (lines, failed) = project_check(&dir).unwrap();
        assert!(failed);
        assert!(lines[0].starts_with("project manifest:"), "{lines:?}");
        fs::write(
            dir.join("tpt-shell.toml"),
            RUST_MANIFEST.replace("{{name}}", "ok"),
        )
        .unwrap();
        let (lines, failed) = project_check(&dir).unwrap();
        assert!(!failed);
        assert_eq!(lines, vec!["project manifest: ok".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn crate_names_follow_cargo_rules() {
        assert!(valid_crate_name("my-app_2"));
        assert!(!valid_crate_name(""));
        assert!(!valid_crate_name("My-app"));
        assert!(!valid_crate_name("2app"));
        assert!(!valid_crate_name("my app"));
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
