// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Boot integration (spec §10.1): the files and commands that start a supervised app at login.
//!
//! - Windows: a Task Scheduler task that runs at logon (`schtasks`).
//! - macOS: a LaunchAgent property list in `~/Library/LaunchAgents`.
//! - Linux: a systemd user unit in `~/.config/systemd/user`.
//!
//! The file writers take the target folder as an argument, so tests write into a temporary
//! folder and never touch the real user configuration. The CLI passes the user's home folder.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A program to start at login, with its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Short name: letters, digits, `-`, `_` or `.`, 1 to 64 characters.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl ServiceSpec {
    /// Checks the name. Fails with `InvalidInput` when it would not be a safe file or task name.
    pub fn new(name: &str, program: impl Into<PathBuf>, args: Vec<String>) -> io::Result<Self> {
        let ok = !name.is_empty()
            && name.len() <= 64
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !ok {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("`{name}` must be 1-64 letters, digits, `-`, `_` or `.`"),
            ));
        }
        Ok(ServiceSpec {
            name: name.to_string(),
            program: program.into(),
            args,
        })
    }

    /// The LaunchAgent label, and the file name without `.plist`.
    pub fn label(&self) -> String {
        format!("com.tpt-shell.{}", self.name)
    }

    /// The scheduled task name on Windows.
    pub fn task_name(&self) -> String {
        format!("tpt-shell-{}", self.name)
    }

    /// The systemd unit name, without the folder.
    pub fn unit_name(&self) -> String {
        format!("tpt-shell-{}.service", self.name)
    }

    /// The program and arguments as one list, the way a launcher runs them.
    fn command(&self) -> Vec<String> {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(self.args.iter().cloned())
            .collect()
    }
}

/// The LaunchAgents folder under a home folder.
pub fn launch_agents_dir(home: &Path) -> PathBuf {
    home.join("Library").join("LaunchAgents")
}

/// The systemd user unit folder under a home folder.
pub fn systemd_user_dir(home: &Path) -> PathBuf {
    home.join(".config").join("systemd").join("user")
}

/// The LaunchAgent property list for `spec`. It starts at login, and launchd restarts it when
/// it exits with a failure.
pub fn launch_agent_plist(spec: &ServiceSpec) -> String {
    let mut args = String::new();
    for part in spec.command() {
        args.push_str(&format!("    <string>{}</string>\n", xml_escape(&part)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n\
  <string>{label}</string>\n\
  <key>ProgramArguments</key>\n\
  <array>\n\
{args}  </array>\n\
  <key>RunAtLoad</key>\n\
  <true/>\n\
  <key>KeepAlive</key>\n\
  <dict>\n\
    <key>SuccessfulExit</key>\n\
    <false/>\n\
  </dict>\n\
  <key>ProcessType</key>\n\
  <string>Interactive</string>\n\
</dict>\n\
</plist>\n",
        label = xml_escape(&spec.label()),
    )
}

/// Writes the LaunchAgent for `spec` into `dir` (normally [`launch_agents_dir`]). Creates the
/// folder if needed. Returns the path written.
pub fn write_launch_agent(dir: &Path, spec: &ServiceSpec) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.plist", spec.label()));
    write_atomically(&path, launch_agent_plist(spec).as_bytes())?;
    Ok(path)
}

/// The systemd user unit for `spec`. It starts with the user's session and restarts on failure.
pub fn systemd_unit(spec: &ServiceSpec) -> String {
    let exec = spec
        .command()
        .iter()
        .map(|part| systemd_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "[Unit]\n\
Description=tpt-shell app {name}\n\
After=graphical-session.target\n\
\n\
[Service]\n\
ExecStart={exec}\n\
Restart=on-failure\n\
RestartSec=5\n\
\n\
[Install]\n\
WantedBy=default.target\n",
        name = spec.name,
    )
}

/// Writes the systemd user unit for `spec` into `dir` (normally [`systemd_user_dir`]).
pub fn write_systemd_unit(dir: &Path, spec: &ServiceSpec) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(spec.unit_name());
    write_atomically(&path, systemd_unit(spec).as_bytes())?;
    Ok(path)
}

/// Arguments for `schtasks` that create a logon task for `spec`. Run them with [`run_schtasks`].
pub fn schtasks_create_args(spec: &ServiceSpec) -> Vec<OsString> {
    let command = spec
        .command()
        .iter()
        .map(|part| windows_quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    [
        "/Create",
        "/TN",
        &spec.task_name(),
        "/TR",
        &command,
        "/SC",
        "ONLOGON",
        "/RL",
        "LIMITED",
        "/F",
    ]
    .iter()
    .map(OsString::from)
    .collect()
}

/// Arguments for `schtasks` that delete the task for `spec`.
pub fn schtasks_delete_args(spec: &ServiceSpec) -> Vec<OsString> {
    ["/Delete", "/TN", &spec.task_name(), "/F"]
        .iter()
        .map(OsString::from)
        .collect()
}

/// Runs `schtasks` with `args`. Windows only; elsewhere it is an `Unsupported` error.
pub fn run_schtasks(args: &[OsString]) -> io::Result<()> {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("schtasks").args(args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("schtasks exited with {status}")))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "schtasks runs on Windows only",
        ))
    }
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Quotes one word for a systemd `ExecStart=` line. `%` and `$` are doubled, as systemd needs.
fn systemd_quote(word: &str) -> String {
    let escaped = word
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
        .replace('$', "$$");
    if word.is_empty() || word.contains([' ', '\t', '"', '\\', '\'']) {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

/// Quotes one word for the `/TR` command line of `schtasks`.
fn windows_quote(word: &str) -> String {
    if word.is_empty() || word.contains([' ', '\t', '"']) {
        format!("\"{}\"", word.replace('"', "\\\""))
    } else {
        word.to_string()
    }
}

/// Writes through a temporary file and a rename, so a partial file is never left behind.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    let tmp = PathBuf::from(name);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn spec() -> ServiceSpec {
        ServiceSpec::new(
            "kiosk-1",
            "/opt/tpt/tpt-shell-supervisor",
            vec![
                "--restart-at".into(),
                "03:00".into(),
                "--".into(),
                "/opt/tpt/app & co".into(),
            ],
        )
        .expect("spec")
    }

    #[test]
    fn names_are_checked_before_they_reach_a_file_or_a_command() {
        assert!(ServiceSpec::new("ok-name_1.2", "p", vec![]).is_ok());
        for bad in [
            "",
            "..",
            "a/b",
            "a b",
            "x\\y",
            "semi;colon",
            &"a".repeat(65),
        ] {
            assert!(
                ServiceSpec::new(bad, "p", vec![]).is_err(),
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn the_plist_has_the_label_program_and_escaped_arguments() {
        let text = launch_agent_plist(&spec());
        assert!(text.contains("<string>com.tpt-shell.kiosk-1</string>"));
        assert!(text.contains("<string>/opt/tpt/tpt-shell-supervisor</string>"));
        assert!(text.contains("<string>/opt/tpt/app &amp; co</string>"));
        assert!(text.contains("<key>RunAtLoad</key>"));
        assert!(text.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn the_launch_agent_is_written_under_the_given_folder_only() {
        let home = scratch("launchagent");
        let dir = launch_agents_dir(&home);
        let path = write_launch_agent(&dir, &spec()).expect("write");
        assert_eq!(path, dir.join("com.tpt-shell.kiosk-1.plist"));
        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text, launch_agent_plist(&spec()));
        assert!(!dir.join("com.tpt-shell.kiosk-1.plist.tmp").exists());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn the_systemd_unit_quotes_words_and_restarts_on_failure() {
        let unit = systemd_unit(&spec());
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(
            unit.contains("ExecStart=/opt/tpt/tpt-shell-supervisor --restart-at 03:00 -- \"/opt/tpt/app & co\""),
            "{unit}"
        );
    }

    #[test]
    fn systemd_percent_and_dollar_are_escaped() {
        let odd = ServiceSpec::new("x", "/bin/app", vec!["100%$HOME".into()]).expect("spec");
        assert!(systemd_unit(&odd).contains("ExecStart=/bin/app 100%%$$HOME"));
    }

    #[test]
    fn the_unit_is_written_under_the_given_folder_only() {
        let home = scratch("systemd");
        let dir = systemd_user_dir(&home);
        let path = write_systemd_unit(&dir, &spec()).expect("write");
        assert_eq!(path, dir.join("tpt-shell-kiosk-1.service"));
        assert_eq!(
            fs::read_to_string(&path).expect("read"),
            systemd_unit(&spec())
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn schtasks_arguments_make_a_logon_task_and_delete_it() {
        let create: Vec<String> = schtasks_create_args(&spec())
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(create[0], "/Create");
        assert!(create.contains(&"tpt-shell-kiosk-1".to_string()));
        assert!(create.contains(&"ONLOGON".to_string()));
        assert!(create.contains(&"/F".to_string()));
        let tr = create.iter().position(|a| a == "/TR").expect("/TR");
        assert_eq!(
            create[tr + 1],
            "/opt/tpt/tpt-shell-supervisor --restart-at 03:00 -- \"/opt/tpt/app & co\""
        );

        let delete: Vec<String> = schtasks_delete_args(&spec())
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(delete, vec!["/Delete", "/TN", "tpt-shell-kiosk-1", "/F"]);
    }
}
