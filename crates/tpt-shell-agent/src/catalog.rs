// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Tools generated from the typed contract (spec §10.4). A command flagged `agent = true` in the
//! contract becomes an agent tool, if the active profile allows it. The contract macro owns the
//! flag; this module takes it from a [`CommandCatalog`] and does not read the contract itself.
//!
//! Generation fails closed: a command the profile does not name is not offered, and a command
//! that is not flagged is never offered, whatever the profile says.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use serde_json::Value;
use tpt_shell_ipc::schema::Schema;

use crate::profile::AgentProfile;
use crate::AgentTool;

/// What the contract says about one command.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandDescriptor {
    pub name: String,
    pub description: String,
    pub args: Schema,
    /// The contract flags this command `agent = true`.
    pub agent: bool,
    /// The contract says this command needs native confirmation.
    pub confirm: bool,
}

/// Lists the commands of a contract. Implemented from the generated contract, so this crate
/// does not depend on the macro crate.
///
/// Support: Full.
pub trait CommandCatalog {
    fn commands(&self) -> Vec<CommandDescriptor>;
}

/// Runs a command by name: the command's name and its arguments, and the result or the error.
///
/// Support: Full.
pub type Runner = Box<dyn FnMut(&str, &Value) -> Result<Value, String>>;

/// Runs a command by name. Shared by every generated tool.
///
/// Support: Full.
pub type Dispatch = Rc<RefCell<Runner>>;

/// The tools for one session, and the commands that need confirmation.
///
/// Support: Full.
pub struct Generated {
    pub tools: Vec<AgentTool>,
    /// Commands the contract marks for confirmation. Merge into the session policy.
    pub confirm: BTreeSet<String>,
}

/// Builds one tool for each agent-flagged command the profile allows. With no profile, every
/// flagged command is offered.
///
/// Support: Full.
pub fn tools_from_catalog(
    catalog: &dyn CommandCatalog,
    profile: Option<&AgentProfile>,
    dispatch: Dispatch,
) -> Generated {
    let mut tools = Vec::new();
    let mut confirm = BTreeSet::new();
    for command in catalog.commands() {
        if !command.agent {
            continue;
        }
        if let Some(profile) = profile {
            if !profile.allows_command(&command.name) {
                continue;
            }
        }
        if command.confirm {
            confirm.insert(command.name.clone());
        }
        let shared = dispatch.clone();
        let name = command.name.clone();
        tools.push(AgentTool {
            command: command.name,
            description: command.description,
            args: command.args,
            handler: Box::new(move |args: &Value| (*shared.borrow_mut())(&name, args)),
        });
    }
    Generated { tools, confirm }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::parse_agent_section;
    use serde_json::json;
    use std::cell::Cell;
    use toml::Table;
    use tpt_shell_ipc::schema::Field;

    const ARGS: &[Field] = &[Field {
        name: "level",
        schema: Schema::Int { min: 0, max: 10 },
    }];

    struct Contract;

    impl CommandCatalog for Contract {
        fn commands(&self) -> Vec<CommandDescriptor> {
            vec![
                CommandDescriptor {
                    name: "set_volume".into(),
                    description: "set the volume".into(),
                    args: Schema::Struct(ARGS),
                    agent: true,
                    confirm: false,
                },
                CommandDescriptor {
                    name: "wipe".into(),
                    description: "wipe".into(),
                    args: Schema::Struct(ARGS),
                    agent: true,
                    confirm: true,
                },
                CommandDescriptor {
                    name: "internal_sync".into(),
                    description: "not for agents".into(),
                    args: Schema::Struct(ARGS),
                    agent: false,
                    confirm: false,
                },
            ]
        }
    }

    fn dispatch(calls: Rc<Cell<u32>>) -> Dispatch {
        let run: Runner = Box::new(move |name: &str, args: &Value| {
            calls.set(calls.get() + 1);
            Ok(json!({ "ran": name, "args": args }))
        });
        Rc::new(RefCell::new(run))
    }

    #[test]
    fn only_agent_flagged_commands_become_tools() {
        let calls = Rc::new(Cell::new(0));
        let generated = tools_from_catalog(&Contract, None, dispatch(calls.clone()));
        let names: Vec<&str> = generated.tools.iter().map(|t| t.command.as_str()).collect();
        assert_eq!(names, vec!["set_volume", "wipe"]);
        assert!(generated.confirm.contains("wipe"));
        assert!(!generated.confirm.contains("set_volume"));
    }

    #[test]
    fn a_tool_calls_the_dispatcher_with_its_name() {
        let calls = Rc::new(Cell::new(0));
        let mut generated = tools_from_catalog(&Contract, None, dispatch(calls.clone()));
        let reply = (generated.tools[0].handler)(&json!({ "level": 2 })).unwrap();
        assert_eq!(reply["ran"], "set_volume");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn a_profile_limits_which_commands_are_offered() {
        let table: Table = "enabled = true\n[profiles.p]\nactions = [\"invoke\"]\ncommands = [\"set_volume\", \"internal_sync\"]\n"
            .parse()
            .unwrap();
        let config = parse_agent_section(Some(&table)).unwrap();
        let profile = config.profile("p").unwrap();
        let generated =
            tools_from_catalog(&Contract, Some(profile), dispatch(Rc::new(Cell::new(0))));
        let names: Vec<&str> = generated.tools.iter().map(|t| t.command.as_str()).collect();
        assert_eq!(
            names,
            vec!["set_volume"],
            "internal_sync is not flagged, so it stays out"
        );
    }
}
