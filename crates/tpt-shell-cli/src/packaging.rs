// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Packaging commands that wrap the scripts under `tools/`: `runtime pin | list`, `vendor` and
//! `docs --offline`. The scripts do the work, so their behaviour and their checks stay in one
//! place. This module finds them, passes the arguments, and reports the exit status. It also
//! holds the optional tool and minimum-version checks that `doctor` prints. Those are warnings
//! and never fail the check.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

const RUNTIME_USAGE: &str =
    "usage: tpt-shell runtime pin <tpt-shell.toml> (--cab <file> | --url <https-url>) --out <dir>
       tpt-shell runtime list <dir>...";

const VENDOR_USAGE: &str = "usage: tpt-shell vendor [directory]";

const DOCS_USAGE: &str = "usage: tpt-shell docs --offline [output.zip]";

/// Finds `relative` (for example `tools/vendor.sh`) in the current folder or one of its parents,
/// so the commands work from anywhere in the repository.
///
/// Support: Partial. Searches the current folder and its parents; no test calls it directly.
pub fn find_repo_file(relative: &str) -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The bash to run the scripts with. On Windows, Git for Windows is preferred, because the
/// `bash` on PATH may be the WSL launcher, which cannot see the Windows folders the scripts use.
fn bash() -> Command {
    if cfg!(windows) {
        for candidate in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
        ] {
            if Path::new(candidate).is_file() {
                return Command::new(candidate);
            }
        }
    }
    Command::new("bash")
}

/// A path as bash takes it: forward slashes, so Git Bash reads a Windows path unchanged.
fn bash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// A path made absolute from the current folder, so a script that changes folder still gets the
/// place the user meant.
fn absolute(path: &str) -> Result<String, String> {
    let current =
        std::env::current_dir().map_err(|e| format!("cannot read the current folder: {e}"))?;
    Ok(bash_path(&current.join(path)))
}

fn exit_code(status: ExitStatus) -> ExitCode {
    match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::from(2),
    }
}

/// Runs `tools/<name>` with `args` through bash, and returns its exit status.
fn run_script(relative: &str, args: &[String]) -> Result<ExitCode, String> {
    let script = find_repo_file(relative)
        .ok_or_else(|| format!("cannot find {relative}; run this from the tpt-shell repository"))?;
    let status = bash()
        .arg(bash_path(&script))
        .args(args)
        .status()
        .map_err(|e| format!("cannot run bash for {relative}: {e} (install Git for Windows)"))?;
    Ok(exit_code(status))
}

/// `runtime pin | list`.
///
/// Support: Partial. Argument checks are tested. The shell script it runs under Git Bash was not
/// run end to end here, and not on macOS or Linux.
pub fn runtime(args: &[&str]) -> Result<ExitCode, String> {
    match args {
        ["list", dirs @ ..] if !dirs.is_empty() => {
            let mut script_args = vec!["list".to_string()];
            script_args.extend(dirs.iter().map(|d| (*d).to_string()));
            run_script("tools/runtime-pin.sh", &script_args)
        }
        ["pin", manifest, rest @ ..] => runtime_pin(Path::new(manifest), rest),
        _ => Err(RUNTIME_USAGE.into()),
    }
}

/// Reads `[runtime.webview2]`. Returns the CAB digest the manifest records, when it holds a real
/// one (the all-zero placeholder is not a digest). Fails unless the mode is `fixed`.
fn webview2_pin(manifest_text: &str) -> Result<Option<String>, String> {
    let table: toml::Table = manifest_text
        .parse()
        .map_err(|e| format!("the manifest is not valid TOML: {e}"))?;
    let section = table
        .get("runtime")
        .and_then(|r| r.get("webview2"))
        .and_then(toml::Value::as_table)
        .ok_or("the manifest has no [runtime.webview2] section")?;
    let mode = section
        .get("mode")
        .and_then(toml::Value::as_str)
        .unwrap_or("evergreen");
    if mode != "fixed" {
        return Err(format!(
            "[runtime.webview2] mode is \"{mode}\"; set mode = \"fixed\" to pin a runtime"
        ));
    }
    let digest = section
        .get("cab_sha256")
        .and_then(toml::Value::as_str)
        .filter(|d| !d.bytes().all(|b| b == b'0'));
    if let Some(d) = digest {
        if d.len() != 64 || !d.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("cab_sha256 in the manifest is not 64 hex digits".into());
        }
    }
    Ok(digest.map(str::to_string))
}

fn runtime_pin(manifest: &Path, rest: &[&str]) -> Result<ExitCode, String> {
    let text = fs::read_to_string(manifest)
        .map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;
    let recorded_digest = webview2_pin(&text)?;

    let mut cab = None;
    let mut url = None;
    let mut out = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        let Some(value) = args.next() else {
            return Err(format!("{arg} needs a value\n{RUNTIME_USAGE}"));
        };
        match *arg {
            "--cab" => cab = Some(*value),
            "--url" => url = Some(*value),
            "--out" => out = Some(*value),
            _ => return Err(RUNTIME_USAGE.into()),
        }
    }
    let out =
        out.ok_or_else(|| format!("runtime pin needs --out <runtime-dir>\n{RUNTIME_USAGE}"))?;
    let mut script_args = vec!["pin".to_string()];
    match (cab, url) {
        (Some(cab), None) => {
            script_args.push("--cab".into());
            script_args.push(absolute(cab)?);
        }
        (None, Some(url)) => {
            script_args.push("--url".into());
            script_args.push(url.to_string());
        }
        _ => {
            return Err(format!(
                "give --cab or --url, not both or neither\n{RUNTIME_USAGE}"
            ))
        }
    }
    if let Some(digest) = recorded_digest {
        script_args.push("--sha256".into());
        script_args.push(digest);
    }
    script_args.push("--out".into());
    script_args.push(absolute(out)?);
    run_script("tools/runtime-pin.sh", &script_args)
}

/// `vendor [directory]`: runs `tools/vendor.sh`.
///
/// Support: Partial. Argument checks are tested. The shell script it runs under Git Bash was not
/// run end to end here, and not on macOS or Linux.
pub fn vendor(args: &[&str]) -> Result<ExitCode, String> {
    let script_args = match args {
        [] => Vec::new(),
        [dir] if !dir.starts_with("--") => vec![absolute(dir)?],
        _ => return Err(VENDOR_USAGE.into()),
    };
    run_script("tools/vendor.sh", &script_args)
}

/// `docs --offline [output.zip]`: runs `tools/docs-offline.sh`. The flag is required, so the
/// command never builds the docs by accident.
///
/// Support: Partial. Argument checks are tested. The shell script it runs under Git Bash was not
/// run end to end here, and not on macOS or Linux.
pub fn docs(args: &[&str]) -> Result<ExitCode, String> {
    let script_args = match args {
        ["--offline"] => Vec::new(),
        ["--offline", zip] if !zip.starts_with("--") => vec![absolute(zip)?],
        _ => return Err(DOCS_USAGE.into()),
    };
    run_script("tools/docs-offline.sh", &script_args)
}

/// The optional tools `doctor` looks for: the program name, and what it is needed for. The
/// signing tools are in `signing_tool_lines`, which takes its lookup as a parameter.
const OPTIONAL_TOOLS: [(&str, &str); 5] = [
    ("codesign", "to sign macOS builds"),
    ("notarytool", "to notarise macOS builds"),
    ("ISCC", "to build the Windows installer (Inno Setup)"),
    ("nfpm", "to build .deb and .rpm packages"),
    ("appimagetool", "to build the Linux AppImage"),
];

/// Whether `program` is on PATH. On Windows, `.exe` is tried as well.
///
/// Support: Full.
pub fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: Vec<String> = if cfg!(windows) {
        vec![program.to_string(), format!("{program}.exe")]
    } else {
        vec![program.to_string()]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|n| dir.join(n).is_file()))
}

/// The value of `${NAME:=value}` in `packaging/config.env`, or `None` when the file does not
/// define it.
///
/// Support: Full.
pub fn config_value(text: &str, name: &str) -> Option<String> {
    let marker = format!("${{{name}:=");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find('}')?;
    Some(rest[..end].trim_matches('"').to_string())
}

/// Whether the dotted version `actual` is at least `minimum`. Non-numeric suffixes are ignored,
/// so `14.5.1` and `2.40-beta` compare by their numbers.
///
/// Support: Full.
pub fn version_at_least(actual: &str, minimum: &str) -> bool {
    let numbers = |s: &str| -> Vec<u64> {
        s.trim()
            .split('.')
            .map(|part| {
                part.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (a, m) = (numbers(actual), numbers(minimum));
    for i in 0..a.len().max(m.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = m.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    true
}

/// The lines `doctor` prints for the signing tools that the sign step needs. `signtool` is needed
/// only on Windows; `minisign` is needed on every platform. Each missing tool carries a fix hint.
/// `found` answers whether a program is available, so tests can give their own answers.
///
/// Support: Full. Detection is tested with an injected lookup; the real lookup is `on_path`.
pub fn signing_tool_lines(os: &str, found: impl Fn(&str) -> bool) -> Vec<String> {
    let mut lines = Vec::new();
    if os == "windows" {
        lines.push(tool_line(
            &found,
            "signtool",
            "to sign Windows builds",
            "install the Windows SDK and add its bin folder to PATH",
        ));
    }
    lines.push(tool_line(
        &found,
        "minisign",
        "to sign release archives",
        "install it from https://jedisct1.github.io/minisign/ and open a new terminal",
    ));
    lines
}

/// One line for an optional tool: its name and whether it was found, with the purpose and a fix
/// hint when it was not.
fn tool_line(found: &impl Fn(&str) -> bool, tool: &str, purpose: &str, hint: &str) -> String {
    if found(tool) {
        format!("{tool}: found")
    } else {
        format!("{tool}: not found (optional, {purpose}; fix: {hint})")
    }
}

/// The lines `doctor` prints for the packaging tools and the platform minimums. Never a failure.
///
/// Support: Full.
pub fn doctor_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for (tool, purpose) in OPTIONAL_TOOLS {
        if on_path(tool) {
            lines.push(format!("{tool}: found"));
        } else {
            lines.push(format!("{tool}: not found (optional, {purpose})"));
        }
    }
    lines.extend(signing_tool_lines(std::env::consts::OS, on_path));
    let Some(config) = find_repo_file("packaging/config.env") else {
        lines.push("platform minimums: packaging/config.env not found, not checked".into());
        return lines;
    };
    let Ok(text) = fs::read_to_string(&config) else {
        lines.push(format!(
            "platform minimums: cannot read {}",
            config.display()
        ));
        return lines;
    };
    if cfg!(target_os = "macos") {
        if let Some(minimum) = config_value(&text, "MIN_MACOS") {
            lines.push(macos_line(&minimum));
        }
    }
    if cfg!(target_os = "linux") {
        if let Some(minimum) = config_value(&text, "MIN_WEBKITGTK") {
            lines.push(webkitgtk_line(&minimum));
        }
    }
    lines
}

fn macos_line(minimum: &str) -> String {
    let output = Command::new("sw_vers").arg("-productVersion").output();
    match output {
        Ok(out) if out.status.success() => {
            let actual = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if version_at_least(&actual, minimum) {
                format!("macOS: {actual} (minimum {minimum} from packaging/config.env)")
            } else {
                format!("warning: macOS {actual} is below the minimum {minimum} in packaging/config.env")
            }
        }
        _ => "macOS version: sw_vers did not run, not checked".into(),
    }
}

fn webkitgtk_line(minimum: &str) -> String {
    let output = Command::new("pkg-config")
        .args(["--modversion", "webkit2gtk-4.1"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let actual = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if version_at_least(&actual, minimum) {
                format!("WebKitGTK: {actual} (minimum {minimum} from packaging/config.env)")
            } else {
                format!("warning: WebKitGTK {actual} is below the minimum {minimum} in packaging/config.env")
            }
        }
        _ => format!(
            "warning: WebKitGTK 4.1 not found (minimum {minimum} from packaging/config.env)"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signtool_is_checked_only_on_windows() {
        let none = |_: &str| false;
        let windows = signing_tool_lines("windows", none);
        assert_eq!(windows.len(), 2);
        assert!(windows[0].starts_with("signtool: not found"));
        assert!(windows[0].contains("Windows SDK"));
        let linux = signing_tool_lines("linux", none);
        assert_eq!(linux.len(), 1);
        assert!(linux[0].starts_with("minisign: not found"));
    }

    #[test]
    fn a_found_tool_has_no_fix_hint() {
        let only_minisign = |t: &str| t == "minisign";
        let lines = signing_tool_lines("linux", only_minisign);
        assert_eq!(lines, vec!["minisign: found".to_string()]);
    }

    #[test]
    fn a_missing_tool_names_its_fix_and_is_not_a_failure() {
        let lines = signing_tool_lines("linux", |_| false);
        assert!(lines[0].contains("jedisct1.github.io/minisign"));
        assert!(lines.iter().all(|l| !l.contains("fail")));
    }

    #[test]
    fn runtime_usage_errors_are_refused() {
        assert!(runtime(&[]).is_err());
        assert!(runtime(&["list"]).is_err());
        assert!(runtime(&["pin"]).is_err());
        assert!(runtime(&["bogus", "x"]).is_err());
    }

    #[test]
    fn a_pin_needs_the_fixed_mode_in_the_manifest() {
        let evergreen = "[runtime.webview2]\nmode = \"evergreen\"\n";
        assert!(webview2_pin(evergreen).is_err());
        assert!(webview2_pin("[app]\nname = \"x\"\n").is_err());
        assert!(webview2_pin("not = [toml").is_err());
    }

    #[test]
    fn the_placeholder_digest_is_not_a_digest() {
        let placeholder = format!(
            "[runtime.webview2]\nmode = \"fixed\"\ncab_sha256 = \"{}\"\n",
            "0".repeat(64)
        );
        assert_eq!(webview2_pin(&placeholder).unwrap(), None);
        let real = format!(
            "[runtime.webview2]\nmode = \"fixed\"\ncab_sha256 = \"{}\"\n",
            "ab".repeat(32)
        );
        assert_eq!(webview2_pin(&real).unwrap(), Some("ab".repeat(32)));
        let short = "[runtime.webview2]\nmode = \"fixed\"\ncab_sha256 = \"abc\"\n";
        assert!(webview2_pin(short).is_err());
    }

    #[test]
    fn pin_flags_are_checked_before_any_script_runs() {
        let dir = std::env::temp_dir().join(format!("tpt-cli-pkg-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("tpt-shell.toml");
        fs::write(&manifest, "[runtime.webview2]\nmode = \"fixed\"\n").unwrap();
        let m = manifest.to_str().unwrap();
        assert!(runtime(&["pin", m]).is_err(), "no --out or CAB");
        assert!(runtime(&["pin", m, "--out", "o"]).is_err(), "no CAB or URL");
        assert!(runtime(&["pin", m, "--cab", "a", "--url", "https://x", "--out", "o"]).is_err());
        assert!(
            runtime(&["pin", m, "--cab"]).is_err(),
            "a flag without a value"
        );
        assert!(runtime(&["pin", m, "--cab", "a", "--out", "o", "--fast"]).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn vendor_and_docs_refuse_stray_arguments() {
        assert!(vendor(&["a", "b"]).is_err());
        assert!(vendor(&["--fast"]).is_err());
        assert!(docs(&[]).is_err(), "--offline is required");
        assert!(docs(&["--online"]).is_err());
        assert!(docs(&["--offline", "a", "b"]).is_err());
    }

    #[test]
    fn config_values_are_read_from_the_shell_defaults() {
        let text = "# comment\n: \"${MIN_MACOS:=12.0}\"\n: \"${MIN_WEBKITGTK:=2.40}\"\n";
        assert_eq!(config_value(text, "MIN_MACOS").as_deref(), Some("12.0"));
        assert_eq!(config_value(text, "MIN_WEBKITGTK").as_deref(), Some("2.40"));
        assert_eq!(
            config_value(text, "MIN_WINDOWS"),
            None,
            "undefined is skipped"
        );
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(version_at_least("14.5", "12.0"));
        assert!(version_at_least("12", "12.0"));
        assert!(!version_at_least("11.7.10", "12.0"));
        assert!(version_at_least("2.44.1", "2.40"));
        assert!(!version_at_least("2.38", "2.40"));
        assert!(version_at_least("2.40-beta", "2.40"));
    }

    #[test]
    fn doctor_lines_never_claim_a_failure() {
        let lines = doctor_lines();
        assert!(lines.len() >= OPTIONAL_TOOLS.len());
        assert!(lines.iter().all(|l| !l.contains("fail")));
    }
}
