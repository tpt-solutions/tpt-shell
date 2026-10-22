// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell permissions suggest`: the smallest set of capability grants that the app's
//! commands need, read from its command contract.
//!
//! It looks at commands only. A page that calls a command gets that command's capability, so
//! the list is complete for the commands. It does not see what the pages do on their own.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use tpt_shell_ipc::contract_load;

/// Prints the grants the contract at `json` needs, with the commands that need each one.
pub fn suggest(json: &Path) -> Result<ExitCode, String> {
    let text = std::fs::read_to_string(json)
        .map_err(|e| format!("cannot read {}: {e}", json.display()))?;
    let contract = contract_load::from_json(&text)?;
    let report = grants(&contract);
    if report.is_empty() {
        println!("no command needs a capability; the grant list can be empty");
        println!("[capabilities]\ngrant = []");
        return Ok(ExitCode::SUCCESS);
    }
    println!("capabilities the commands need:");
    for (capability, commands) in &report {
        println!("  {capability:<24} {}", commands.join(", "));
    }
    let names: Vec<String> = report.keys().map(|c| format!("{c:?}")).collect();
    println!(
        "\nadd to tpt-shell.toml:\n[capabilities]\ngrant = [{}]",
        names.join(", ")
    );
    println!("\nThis lists the commands only. Check the pages for anything they do on their own.");
    Ok(ExitCode::SUCCESS)
}

/// Each capability the contract's commands name, with the commands that name it, sorted.
pub fn grants(contract: &tpt_shell_ipc::contract::Contract) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for command in contract.commands() {
        if let Some(capability) = command.capability {
            out.entry(capability.to_string())
                .or_default()
                .push(command.name.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_capability_lists_the_commands_that_need_it() {
        let text = r#"{"contract_version":1,"commands":[
            {"name":"save","kind":"sync","capability":"files.write","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"load","kind":"sync","capability":"files.read","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"copy","kind":"sync","capability":"files.write","args":{"type":"object","properties":{},"required":[],"additionalProperties":false}},
            {"name":"ping","kind":"sync","capability":null,"args":{"type":"object","properties":{},"required":[],"additionalProperties":false}}
        ]}"#;
        let contract = contract_load::from_json(text).unwrap();
        let report = grants(&contract);
        assert_eq!(
            report.keys().collect::<Vec<_>>(),
            ["files.read", "files.write"]
        );
        assert_eq!(report["files.write"], ["copy", "save"]);
        assert_eq!(report["files.read"], ["load"]);
    }
}
