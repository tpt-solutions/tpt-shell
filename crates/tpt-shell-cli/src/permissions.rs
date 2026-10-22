// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell permissions suggest`: the capability grants an app needs, with the commands that
//! need each one. Two sources:
//!
//! - the command contract: every capability a command names. It looks at commands only, so
//!   pages that act on their own are not seen;
//! - an audit log: the calls refused with `CAP_NO_GRANT`, each of which names the capability it
//!   was missing. This is the list of grants the app was actually short of.
//!
//! Neither source widens a grant. The output is text to copy into `tpt-shell.toml`, and the
//! user decides what to add.

use std::collections::BTreeMap;

/// Each capability with the commands that name it, sorted.
pub type Grants = BTreeMap<String, Vec<String>>;

/// Each capability the contract's commands name, with the commands that name it.
pub fn from_contract(contract: &tpt_shell_ipc::contract::Contract) -> Grants {
    let mut out: Grants = BTreeMap::new();
    for command in contract.commands() {
        if let Some(capability) = command.capability {
            out.entry(capability.to_string())
                .or_default()
                .push(command.name.to_string());
        }
    }
    out
}

/// The grants that the audit log's refused calls were missing. Each `CAP_NO_GRANT` record names
/// its capability and command. Other refusals are not manifest fixes: `CAP_DENIED` means the user
/// said no, and `CAP_NOT_DECLARED` means the command itself needs a capability. Each command is
/// listed once per capability, with how many calls were refused for it.
pub fn from_audit(text: &str) -> Result<Grants, String> {
    let mut out: Grants = BTreeMap::new();
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (index, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let record: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("record {}: {e}", index + 1))?;
        if record.get("error_code").and_then(serde_json::Value::as_str) != Some("CAP_NO_GRANT") {
            continue;
        }
        let (Some(capability), Some(command)) = (
            record.get("capability").and_then(serde_json::Value::as_str),
            record.get("command").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        *counts
            .entry((capability.to_string(), command.to_string()))
            .or_default() += 1;
    }
    for ((capability, command), refused) in counts {
        let entry = out.entry(capability).or_default();
        entry.push(format!("{command} ({refused} refused)"));
    }
    Ok(out)
}

/// Prints the grants under `heading`, and the line to copy into the manifest.
pub fn print(grants: &Grants, heading: &str) {
    if grants.is_empty() {
        println!("No capability grant is needed.");
        println!("[capabilities]\ngrant = []");
        return;
    }
    println!("{heading}:");
    for (capability, commands) in grants {
        println!("  {capability:<24} {}", commands.join(", "));
    }
    let names: Vec<String> = grants.keys().map(|c| format!("{c:?}")).collect();
    println!(
        "\nadd to tpt-shell.toml:\n[capabilities]\ngrant = [{}]",
        names.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_shell_ipc::contract_load;

    #[test]
    fn each_capability_lists_the_commands_that_need_it() {
        let text = r#"{"contract_version":1,"commands":[
            {"name":"save","kind":"sync","capability":"files.write","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"load","kind":"sync","capability":"files.read","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"copy","kind":"sync","capability":"files.write","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"ping","kind":"sync","capability":null,"args":{"type":"object","properties":{},"required":[],"additionalProperties":false}}
        ]}"#;
        let contract = contract_load::from_json(text).unwrap();
        let grants = from_contract(&contract);
        assert_eq!(
            grants.keys().collect::<Vec<_>>(),
            ["files.read", "files.write"]
        );
        assert_eq!(grants["files.write"], ["copy", "save"]);
        assert_eq!(grants["files.read"], ["load"]);
    }

    #[test]
    fn only_no_grant_refusals_name_a_missing_grant() {
        let log = concat!(
            r#"{"command":"save","capability":"files.write","error_code":"CAP_NO_GRANT","outcome":"err"}"#,
            "\n",
            r#"{"command":"save","capability":"files.write","error_code":"CAP_NO_GRANT","outcome":"err"}"#,
            "\n",
            r#"{"command":"open","capability":"files.read","error_code":"CAP_DENIED","outcome":"err"}"#,
            "\n",
            r#"{"command":"ping","capability":null,"error_code":null,"outcome":"ok"}"#,
            "\n"
        );
        let grants = from_audit(log).unwrap();
        assert_eq!(grants.keys().collect::<Vec<_>>(), ["files.write"]);
        assert_eq!(grants["files.write"], ["save (2 refused)"]);
    }

    #[test]
    fn a_real_log_is_read_after_it_verifies() {
        use tpt_shell_audit::{AuditLog, Entry, Outcome, Policy};
        let dir = std::env::temp_dir().join(format!("tpt-cli-perms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        let mut log = AuditLog::open(&path, Policy::default()).unwrap();
        log.append(&Entry {
            wall_unix_ms: 1,
            mono_ns: 1,
            actor: "page".into(),
            command: "save".into(),
            capability: Some("files.write".into()),
            params: serde_json::json!({}),
            outcome: Outcome::Err("CAP_NO_GRANT".into()),
            duration_us: 1,
        })
        .unwrap();
        drop(log);
        assert!(tpt_shell_audit::verify_file(&path).is_ok());
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            from_audit(&text).unwrap()["files.write"],
            ["save (1 refused)"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_with_no_refusals_needs_nothing() {
        let grants =
            from_audit(r#"{"command":"ping","capability":null,"error_code":null}"#).unwrap();
        assert!(grants.is_empty());
    }
}
