// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `contract.json`: the commands a shell exposes, with their argument schemas as JSON Schema
//! (spec M2, codegen). Output is deterministic: commands are sorted by name, and object keys
//! come out sorted because `serde_json::Map` is ordered without the `preserve_order` feature.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::schema::Schema;

/// Version of the contract file format.
///
/// Support: Full.
pub const CONTRACT_VERSION: u64 = 1;

/// How a command runs. `Sync` runs to completion in its handler. `Async` is a future that the
/// handler drives with [`crate::block_on`]. `Job` returns a job id at once and reports through
/// the jobs runner (see [`crate::jobs`]). Streams are not a command kind yet.
///
/// Support: Partial. Streams are not a command kind yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sync,
    Async,
    Job,
}

impl Kind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kind::Sync => "sync",
            Kind::Async => "async",
            Kind::Job => "job",
        }
    }
}

/// Flags a command carries besides its kind. Both default to off: a command runs on the worker
/// side unless it says otherwise, and an agent does not see a command unless it is marked for
/// the agent surface (spec §8, §10.4).
///
/// Support: Full.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// Runs on the UI thread (`#[command(ui)]`).
    pub ui: bool,
    /// Exposed on the agent and MCP surface (`#[command(agent)]`).
    pub agent: bool,
}

/// One command's public description.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: &'static str,
    pub kind: Kind,
    /// Capability the command needs, if any.
    pub capability: Option<&'static str>,
    pub args: Schema,
}

/// A set of commands with unique names.
///
/// Support: Full.
#[derive(Default)]
pub struct Contract {
    commands: BTreeMap<&'static str, Command>,
    flags: BTreeMap<&'static str, Flags>,
}

#[derive(Debug, PartialEq, Eq)]
/// Support: Full.
pub struct DuplicateCommand(pub &'static str);

impl Contract {
    /// Support: Full.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a command with default flags.
    ///
    /// Support: Full.
    pub fn add(&mut self, command: Command) -> Result<(), DuplicateCommand> {
        self.add_flagged(command, Flags::default())
    }

    /// Adds a command with its flags.
    ///
    /// Support: Full.
    pub fn add_flagged(&mut self, command: Command, flags: Flags) -> Result<(), DuplicateCommand> {
        if self.commands.contains_key(command.name) {
            return Err(DuplicateCommand(command.name));
        }
        self.flags.insert(command.name, flags);
        self.commands.insert(command.name, command);
        Ok(())
    }

    /// The commands, sorted by name.
    ///
    /// Support: Full.
    pub fn commands(&self) -> impl Iterator<Item = &Command> {
        self.commands.values()
    }

    /// The flags of the command called `name`, or `None` when there is no such command.
    ///
    /// Support: Full.
    pub fn flags(&self, name: &str) -> Option<Flags> {
        self.flags.get(name).copied()
    }

    /// The contract as pretty-printed JSON with a trailing newline.
    ///
    /// Support: Full.
    pub fn to_json_string(&self) -> String {
        let commands: Vec<Value> = self
            .commands
            .values()
            .map(|c| {
                let mut value = command_json(c);
                let flags = self.flags.get(c.name).copied().unwrap_or_default();
                value["flags"] = json!({ "agent": flags.agent, "ui": flags.ui });
                value
            })
            .collect();
        let doc = json!({
            "contract_version": CONTRACT_VERSION,
            "commands": commands,
        });
        let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
        text.push('\n');
        text
    }
}

fn command_json(c: &Command) -> Value {
    json!({
        "name": c.name,
        "kind": c.kind.as_str(),
        "capability": c.capability,
        "args": json_schema(&c.args),
    })
}

/// Converts one [`Schema`] to a JSON Schema (draft 2020-12 keywords).
///
/// Support: Full.
pub fn json_schema(schema: &Schema) -> Value {
    match schema {
        Schema::Bool => json!({ "type": "boolean" }),
        Schema::Int { min, max } => json!({ "type": "integer", "minimum": min, "maximum": max }),
        Schema::Float { min, max } => json!({ "type": "number", "minimum": min, "maximum": max }),
        Schema::Str { max_len, chars } => {
            let mut obj = Map::new();
            obj.insert("type".into(), json!("string"));
            obj.insert("maxLength".into(), json!(max_len));
            if let Some(allowed) = chars {
                obj.insert("pattern".into(), json!(char_class_pattern(allowed)));
            }
            Value::Object(obj)
        }
        Schema::Enum(options) => json!({ "type": "string", "enum": options }),
        Schema::Optional(inner) => json!({ "anyOf": [json_schema(inner), { "type": "null" }] }),
        Schema::List { item, max_len } => {
            json!({ "type": "array", "items": json_schema(item), "maxItems": max_len })
        }
        Schema::Map { value, max_len } => json!({
            "type": "object",
            "additionalProperties": json_schema(value),
            "maxProperties": max_len,
        }),
        Schema::Struct(fields) => {
            let mut properties = Map::new();
            let mut required = Vec::new();
            for field in fields.iter() {
                properties.insert(field.name.into(), json_schema(&field.schema));
                if !matches!(field.schema, Schema::Optional(_)) {
                    required.push(field.name);
                }
            }
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            })
        }
    }
}

/// A regex that accepts only the characters in `allowed`, of any length. Regex metacharacters
/// are escaped, so the set is matched literally.
fn char_class_pattern(allowed: &str) -> String {
    let mut class = String::new();
    for c in allowed.chars() {
        if matches!(c, '\\' | ']' | '[' | '^' | '-') {
            class.push('\\');
        }
        class.push(c);
    }
    format!("^[{class}]*$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;

    const SLUG: &[Field] = &[Field {
        name: "label",
        schema: Schema::Str {
            max_len: 16,
            chars: Some("ab-"),
        },
    }];

    fn sample() -> Contract {
        let mut c = Contract::new();
        c.add(Command {
            name: "zeta",
            kind: Kind::Sync,
            capability: None,
            args: Schema::Struct(&[]),
        })
        .unwrap();
        c.add(Command {
            name: "alpha",
            kind: Kind::Sync,
            capability: Some("fs.read"),
            args: Schema::Struct(&[
                Field {
                    name: "count",
                    schema: Schema::Int { min: 0, max: 9 },
                },
                Field {
                    name: "note",
                    schema: Schema::Optional(&Schema::Bool),
                },
                Field {
                    name: "slug",
                    schema: Schema::Struct(SLUG),
                },
            ]),
        })
        .unwrap();
        c
    }

    #[test]
    fn duplicate_names_are_refused() {
        let mut c = sample();
        let again = Command {
            name: "alpha",
            kind: Kind::Sync,
            capability: None,
            args: Schema::Bool,
        };
        assert_eq!(c.add(again), Err(DuplicateCommand("alpha")));
    }

    #[test]
    fn output_is_deterministic_and_sorted() {
        let a = sample().to_json_string();
        let b = sample().to_json_string();
        assert_eq!(a, b);
        assert!(a.find("\"alpha\"").unwrap() < a.find("\"zeta\"").unwrap());
    }

    #[test]
    fn golden_contract() {
        let expected = r#"{
  "commands": [
    {
      "args": {
        "additionalProperties": false,
        "properties": {
          "count": {
            "maximum": 9,
            "minimum": 0,
            "type": "integer"
          },
          "note": {
            "anyOf": [
              {
                "type": "boolean"
              },
              {
                "type": "null"
              }
            ]
          },
          "slug": {
            "additionalProperties": false,
            "properties": {
              "label": {
                "maxLength": 16,
                "pattern": "^[ab\\-]*$",
                "type": "string"
              }
            },
            "required": [
              "label"
            ],
            "type": "object"
          }
        },
        "required": [
          "count",
          "slug"
        ],
        "type": "object"
      },
      "capability": "fs.read",
      "flags": {
        "agent": false,
        "ui": false
      },
      "kind": "sync",
      "name": "alpha"
    },
    {
      "args": {
        "additionalProperties": false,
        "properties": {},
        "required": [],
        "type": "object"
      },
      "capability": null,
      "flags": {
        "agent": false,
        "ui": false
      },
      "kind": "sync",
      "name": "zeta"
    }
  ],
  "contract_version": 1
}
"#;
        assert_eq!(sample().to_json_string(), expected);
    }

    #[test]
    fn character_class_metacharacters_are_escaped() {
        assert_eq!(char_class_pattern("a]-^\\"), r"^[a\]\-\^\\]*$");
    }
}
