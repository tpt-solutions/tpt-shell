// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Reads a contract written by [`Contract::to_json_string`] back into a [`Contract`], so a tool
//! that has only the JSON (such as `tpt-shell contract gen`) can generate from it.
//!
//! The contract uses `'static` strings and schemas, so the reader leaks the strings and the
//! nested schemas it builds. It runs once per tool invocation, so that is bounded and small.
//!
//! Support: Partial. Reads the forms that [`json_schema`] writes. Other JSON Schema is refused
//! with the path of the first thing it does not understand.

use serde_json::{Map, Value};

use crate::contract::{Command, Contract, Flags, Kind, CONTRACT_VERSION};
use crate::schema::{Field, Schema};

/// Reads a contract from its JSON text.
///
/// Support: Partial. See the module notes.
pub fn from_json(text: &str) -> Result<Contract, String> {
    let doc: Value =
        serde_json::from_str(text).map_err(|e| format!("the contract is not valid JSON: {e}"))?;
    let version = doc.get("contract_version").and_then(Value::as_u64);
    if version != Some(CONTRACT_VERSION) {
        return Err(format!(
            "the contract has contract_version {version:?}; this tool reads version {CONTRACT_VERSION}"
        ));
    }
    let commands = doc
        .get("commands")
        .and_then(Value::as_array)
        .ok_or("the contract has no `commands` array")?;
    let mut contract = Contract::new();
    for entry in commands {
        let name = leak(&text_at(entry, "name", "commands")?);
        let path = format!("command {name}");
        let kind = match text_at(entry, "kind", &path)?.as_str() {
            "sync" => Kind::Sync,
            "async" => Kind::Async,
            "job" => Kind::Job,
            other => return Err(format!("{path}: unknown kind {other:?}")),
        };
        let capability = match entry.get("capability") {
            None | Some(Value::Null) => None,
            Some(value) => Some(leak(
                value
                    .as_str()
                    .ok_or(format!("{path}: capability is not text"))?,
            )),
        };
        let args = schema(
            entry
                .get("args")
                .ok_or(format!("{path}: no `args` schema"))?,
            &format!("{path} args"),
        )?;
        let flags = match entry.get("flags") {
            None | Some(Value::Null) => Flags::default(),
            Some(value) => Flags {
                agent: flag(value, "agent", &path)?,
                ui: flag(value, "ui", &path)?,
            },
        };
        let command = Command {
            name,
            kind,
            capability,
            args,
        };
        contract
            .add_flagged(command, flags)
            .map_err(|e| format!("the command {} appears twice", e.0))?;
    }
    Ok(contract)
}

/// One schema, from its JSON Schema form. `path` names where it is, for error messages.
fn schema(value: &Value, path: &str) -> Result<Schema, String> {
    if let Some(options) = value.get("enum") {
        let options = options
            .as_array()
            .ok_or(format!("{path}: enum is not a list"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(leak)
                    .ok_or(format!("{path}: enum holds a non-text value"))
            })
            .collect::<Result<Vec<&'static str>, String>>()?;
        return Ok(Schema::Enum(Box::leak(options.into_boxed_slice())));
    }
    if let Some(parts) = value.get("anyOf").and_then(Value::as_array) {
        return match parts.as_slice() {
            [inner, null] if null.get("type").and_then(Value::as_str) == Some("null") => {
                Ok(Schema::Optional(leak_schema(inner, path)?))
            }
            _ => Err(format!("{path}: anyOf is only read as an optional value")),
        };
    }
    match value.get("type").and_then(Value::as_str) {
        Some("boolean") => Ok(Schema::Bool),
        Some("integer") => Ok(Schema::Int {
            min: int_at(value, "minimum", path)?,
            max: int_at(value, "maximum", path)?,
        }),
        Some("number") => Ok(Schema::Float {
            min: float_at(value, "minimum", path)?,
            max: float_at(value, "maximum", path)?,
        }),
        Some("string") => Ok(Schema::Str {
            max_len: count_at(value, "maxLength", path)?,
            chars: match value.get("pattern") {
                None => None,
                Some(pattern) => Some(leak(&char_set(
                    pattern
                        .as_str()
                        .ok_or(format!("{path}: pattern is not text"))?,
                    path,
                )?)),
            },
        }),
        Some("array") => Ok(Schema::List {
            item: leak_schema(
                value
                    .get("items")
                    .ok_or(format!("{path}: array has no items"))?,
                path,
            )?,
            max_len: count_at(value, "maxItems", path)?,
        }),
        Some("object") => object(value, path),
        other => Err(format!("{path}: unsupported type {other:?}")),
    }
}

/// An object schema: a struct when it has `properties`, a map when it has `additionalProperties`.
fn object(value: &Value, path: &str) -> Result<Schema, String> {
    if let Some(properties) = value.get("properties") {
        let properties = properties
            .as_object()
            .ok_or(format!("{path}: properties is not an object"))?;
        let fields = properties
            .iter()
            .map(|(name, field)| {
                let field_path = format!("{path}.{name}");
                Ok(Field {
                    name: leak(name),
                    schema: schema(field, &field_path)?,
                })
            })
            .collect::<Result<Vec<Field>, String>>()?;
        return Ok(Schema::Struct(Box::leak(fields.into_boxed_slice())));
    }
    if let Some(value_schema) = value.get("additionalProperties").filter(|v| v.is_object()) {
        return Ok(Schema::Map {
            value: leak_schema(value_schema, path)?,
            max_len: count_at(value, "maxProperties", path)?,
        });
    }
    Err(format!("{path}: unsupported object schema"))
}

/// The characters a `pattern` of [`char_class_pattern`] form accepts. Any other pattern is
/// refused, since its meaning cannot be recovered as a character set.
fn char_set(pattern: &str, path: &str) -> Result<String, String> {
    let class = pattern
        .strip_prefix("^[")
        .and_then(|rest| rest.strip_suffix("]*$"))
        .ok_or(format!(
            "{path}: pattern {pattern:?} is not a character set"
        ))?;
    let mut set = String::new();
    let mut chars = class.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            set.push(
                chars
                    .next()
                    .ok_or(format!("{path}: pattern ends in a backslash"))?,
            );
        } else {
            set.push(c);
        }
    }
    Ok(set)
}

fn leak_schema(value: &Value, path: &str) -> Result<&'static Schema, String> {
    Ok(Box::leak(Box::new(schema(value, path)?)))
}

fn leak(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

fn text_at(value: &Value, key: &str, path: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or(format!("{path}: no text `{key}`"))
}

fn flag(value: &Value, key: &str, path: &str) -> Result<bool, String> {
    let object: &Map<String, Value> = value
        .as_object()
        .ok_or(format!("{path}: flags is not an object"))?;
    match object.get(key) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{path}: flag {key} is not true or false")),
    }
}

fn int_at(value: &Value, key: &str, path: &str) -> Result<i64, String> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or(format!("{path}: no whole number `{key}`"))
}

fn float_at(value: &Value, key: &str, path: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or(format!("{path}: no number `{key}`"))
}

fn count_at(value: &Value, key: &str, path: &str) -> Result<usize, String> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(format!("{path}: no count `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Command;
    use crate::typescript;

    const SLUG: &[Field] = &[
        Field {
            name: "label",
            schema: Schema::Str {
                max_len: 16,
                chars: Some("ab-]"),
            },
        },
        Field {
            name: "note",
            schema: Schema::Optional(&Schema::Bool),
        },
    ];

    fn sample() -> Contract {
        let mut contract = Contract::new();
        contract
            .add(Command {
                name: "zeta",
                kind: Kind::Sync,
                capability: None,
                args: Schema::Struct(&[]),
            })
            .unwrap();
        contract
            .add_flagged(
                Command {
                    name: "slug.make",
                    kind: Kind::Job,
                    capability: Some("files.write"),
                    args: Schema::Struct(SLUG),
                },
                Flags {
                    agent: true,
                    ui: true,
                },
            )
            .unwrap();
        contract
            .add(Command {
                name: "pick",
                kind: Kind::Async,
                capability: None,
                args: Schema::Map {
                    value: &Schema::List {
                        item: &Schema::Enum(&["red", "blue"]),
                        max_len: 3,
                    },
                    max_len: 4,
                },
            })
            .unwrap();
        contract
    }

    #[test]
    fn a_contract_reads_back_to_the_same_json_and_the_same_client() {
        let original = sample();
        let json = original.to_json_string();
        let read = from_json(&json).expect("the written contract reads back");
        assert_eq!(read.to_json_string(), json);
        assert_eq!(typescript::generate(&read), typescript::generate(&original));
        assert_eq!(
            read.flags("slug.make"),
            Some(Flags {
                agent: true,
                ui: true
            })
        );
    }

    #[test]
    fn a_wrong_version_or_a_missing_list_is_refused() {
        assert!(from_json(r#"{"contract_version": 2, "commands": []}"#).is_err());
        assert!(from_json(r#"{"contract_version": 1}"#).is_err());
        assert!(from_json("not json").is_err());
    }

    #[test]
    fn a_duplicate_command_is_refused() {
        let one = r#"{"name":"a","kind":"sync","capability":null,"args":{"type":"object","properties":{},"required":[],"additionalProperties":false}}"#;
        let text = format!(r#"{{"contract_version":1,"commands":[{one},{one}]}}"#);
        assert!(from_json(&text).err().unwrap().contains("appears twice"));
    }

    #[test]
    fn an_unknown_schema_is_refused_with_its_path() {
        let text = r#"{"contract_version":1,"commands":[{"name":"a","kind":"sync","args":{"type":"null"}}]}"#;
        let err = from_json(text).err().unwrap();
        assert!(err.contains("command a args"), "{err}");
    }
}
