// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Compares two `contract.json` files and marks each change as breaking or compatible for a
//! caller that was built against the old contract (spec M3, `contract diff | check`).
//!
//! The rules are written from the caller's side:
//! - A removed command, a changed kind, or a capability that is added or changed breaks
//!   callers. Dropping a capability requirement does not.
//! - The shell rejects fields it does not know, so removing a field breaks callers. A new
//!   required field breaks them, a new optional field does not. A field that becomes
//!   required breaks them, and one that stops being required does not.
//! - A lower maximum, a higher minimum, a changed or new pattern, a removed enum value and a
//!   changed type break callers. The opposite changes do not.
//! - Any other change to an argument schema is reported as breaking until a rule covers it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::contract::CONTRACT_VERSION;

/// One difference between two contracts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Where the change is: a command (`ping`), an argument (`ping.args.name`), and so on.
    pub path: String,
    /// Whether a caller built against the old contract can break.
    pub breaking: bool,
    pub message: String,
}

/// Compares two contract files given as text, returning every change in a stable order.
pub fn diff(old: &str, new: &str) -> Result<Vec<Change>, String> {
    let old = parse(old, "the old contract")?;
    let new = parse(new, "the new contract")?;
    let old = commands(&old)?;
    let new = commands(&new)?;

    let mut out = Vec::new();
    for (name, before) in &old {
        match new.get(name) {
            None => push(&mut out, name, true, "command removed"),
            Some(after) => diff_command(name, before, after, &mut out),
        }
    }
    for name in new.keys().filter(|name| !old.contains_key(*name)) {
        push(&mut out, name, false, "command added");
    }
    Ok(out)
}

fn parse(text: &str, what: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| format!("{what} is not valid JSON: {e}"))?;
    let version = value.get("contract_version").and_then(Value::as_u64);
    if version != Some(CONTRACT_VERSION) {
        return Err(format!(
            "{what} has contract_version {version:?}; this tool reads version {CONTRACT_VERSION}"
        ));
    }
    Ok(value)
}

/// The commands of a parsed contract, keyed by name and sorted.
fn commands(doc: &Value) -> Result<BTreeMap<String, &Value>, String> {
    let list = doc
        .get("commands")
        .and_then(Value::as_array)
        .ok_or("the contract has no `commands` list")?;
    let mut out = BTreeMap::new();
    for command in list {
        let name = command
            .get("name")
            .and_then(Value::as_str)
            .ok_or("a command has no `name`")?;
        if out.insert(name.to_string(), command).is_some() {
            return Err(format!("the contract lists `{name}` twice"));
        }
    }
    Ok(out)
}

fn push(out: &mut Vec<Change>, path: &str, breaking: bool, message: impl Into<String>) {
    out.push(Change {
        path: path.to_string(),
        breaking,
        message: message.into(),
    });
}

fn diff_command(name: &str, old: &Value, new: &Value, out: &mut Vec<Change>) {
    if old.get("kind") != new.get("kind") {
        push(
            out,
            name,
            true,
            format!(
                "kind changed from {} to {}",
                text_of(old.get("kind")),
                text_of(new.get("kind"))
            ),
        );
    }
    let (before, after) = (old.get("capability"), new.get("capability"));
    if before != after {
        if after.is_none_or(Value::is_null) {
            push(out, name, false, "no longer needs a capability");
        } else {
            push(
                out,
                name,
                true,
                format!("now needs capability {}", text_of(after)),
            );
        }
    }
    let args_path = format!("{name}.args");
    diff_schema(
        &args_path,
        old.get("args").unwrap_or(&Value::Null),
        new.get("args").unwrap_or(&Value::Null),
        out,
    );
}

fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "nothing".into(),
    }
}

/// Keywords with their own rules. Any other keyword that differs is breaking.
const HANDLED: [&str; 13] = [
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "maxLength",
    "maxItems",
    "maxProperties",
    "minimum",
    "maximum",
    "pattern",
    "enum",
    "anyOf",
];

fn diff_schema(path: &str, old: &Value, new: &Value, out: &mut Vec<Change>) {
    if old == new {
        return;
    }
    // `Optional` is written as `anyOf [inner, null]`.
    if let Some(options) = new.get("anyOf") {
        if options.as_array().is_some_and(|a| a.contains(old)) {
            push(out, path, false, "now also accepts null");
        } else {
            push(out, path, true, "schema changed");
        }
        return;
    }
    if let Some(options) = old.get("anyOf") {
        if options.as_array().is_some_and(|a| a.contains(new)) {
            push(out, path, true, "no longer accepts null");
        } else {
            push(out, path, true, "schema changed");
        }
        return;
    }
    let (Some(o), Some(n)) = (old.as_object(), new.as_object()) else {
        push(out, path, true, "schema changed");
        return;
    };

    if o.get("type") != n.get("type") {
        push(
            out,
            path,
            true,
            format!(
                "type changed from {} to {}",
                text_of(o.get("type")),
                text_of(n.get("type"))
            ),
        );
    }
    diff_properties(path, o, n, out);
    diff_additional(path, o, n, out);
    if let (Some(a), Some(b)) = (o.get("items"), n.get("items")) {
        diff_schema(&format!("{path}[]"), a, b, out);
    }
    for key in ["maxLength", "maxItems", "maxProperties"] {
        bound(path, key, o.get(key), n.get(key), true, out);
    }
    for key in ["minimum", "maximum"] {
        // A minimum is tighter when raised; a maximum is tighter when lowered.
        bound(path, key, o.get(key), n.get(key), key == "maximum", out);
    }
    pattern(path, o.get("pattern"), n.get("pattern"), out);
    enum_values(path, o.get("enum"), n.get("enum"), out);

    let other: BTreeSet<&String> = o.keys().chain(n.keys()).collect();
    for key in other {
        if HANDLED.contains(&key.as_str()) {
            continue;
        }
        if o.get(key) != n.get(key) {
            push(out, path, true, format!("`{key}` changed"));
        }
    }
}

fn diff_properties(
    path: &str,
    o: &Map<String, Value>,
    n: &Map<String, Value>,
    out: &mut Vec<Change>,
) {
    let empty = Map::new();
    let old_props = o
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let new_props = n
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let old_required = required_names(o);
    let new_required = required_names(n);

    for (field, before) in old_props {
        let field_path = format!("{path}.{field}");
        match new_props.get(field) {
            None => push(out, &field_path, true, "field removed"),
            Some(after) => diff_schema(&field_path, before, after, out),
        }
    }
    for field in new_props.keys().filter(|f| !old_props.contains_key(*f)) {
        let field_path = format!("{path}.{field}");
        if new_required.contains(field.as_str()) {
            push(out, &field_path, true, "new required field");
        } else {
            push(out, &field_path, false, "new optional field");
        }
    }
    for field in new_required.difference(&old_required) {
        if old_props.contains_key(*field) {
            push(
                out,
                &format!("{path}.{field}"),
                true,
                "field is now required",
            );
        }
    }
    for field in old_required.difference(&new_required) {
        if new_props.contains_key(*field) {
            push(
                out,
                &format!("{path}.{field}"),
                false,
                "field is no longer required",
            );
        }
    }
}

fn required_names(schema: &Map<String, Value>) -> BTreeSet<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn diff_additional(
    path: &str,
    o: &Map<String, Value>,
    n: &Map<String, Value>,
    out: &mut Vec<Change>,
) {
    let (before, after) = (o.get("additionalProperties"), n.get("additionalProperties"));
    if before == after {
        return;
    }
    match (before, after) {
        (Some(a), Some(b)) if a.is_object() && b.is_object() => {
            diff_schema(&format!("{path}.*"), a, b, out);
        }
        // No `additionalProperties` at all means any value is accepted.
        (_, None) => push(out, path, false, "accepts any value for extra fields"),
        (None | Some(Value::Bool(true)), Some(Value::Bool(false))) => {
            push(out, path, true, "extra fields are now refused")
        }
        (Some(Value::Bool(false)), Some(_)) => push(out, path, false, "now accepts extra fields"),
        _ => push(out, path, true, "extra-field rule changed"),
    }
}

/// A numeric bound. `lower_is_tighter` is true for a maximum length or count, and for a
/// maximum; it is false for a minimum.
fn bound(
    path: &str,
    key: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    lower_is_tighter: bool,
    out: &mut Vec<Change>,
) {
    match (old, new) {
        (None, None) => {}
        (Some(a), Some(b)) if a == b => {}
        (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => {
                let tighter = if lower_is_tighter { y < x } else { y > x };
                let (breaking, word) = if tighter {
                    (true, "tightened")
                } else {
                    (false, "loosened")
                };
                push(
                    out,
                    path,
                    breaking,
                    format!("`{key}` {word} from {a} to {b}"),
                );
            }
            _ => push(out, path, true, format!("`{key}` changed")),
        },
        (None, Some(b)) => push(out, path, true, format!("`{key}` added as {b}")),
        (Some(_), None) => push(out, path, false, format!("`{key}` removed")),
    }
}

fn pattern(path: &str, old: Option<&Value>, new: Option<&Value>, out: &mut Vec<Change>) {
    match (old, new) {
        (None, None) => {}
        (Some(a), Some(b)) if a == b => {}
        (Some(_), Some(_)) => push(out, path, true, "pattern changed"),
        (None, Some(_)) => push(out, path, true, "pattern added"),
        (Some(_), None) => push(out, path, false, "pattern removed"),
    }
}

fn enum_values(path: &str, old: Option<&Value>, new: Option<&Value>, out: &mut Vec<Change>) {
    match (old.and_then(Value::as_array), new.and_then(Value::as_array)) {
        (None, None) => {}
        (Some(a), Some(b)) => {
            let removed: Vec<String> = a
                .iter()
                .filter(|v| !b.contains(v))
                .map(Value::to_string)
                .collect();
            let added: Vec<String> = b
                .iter()
                .filter(|v| !a.contains(v))
                .map(Value::to_string)
                .collect();
            if !removed.is_empty() {
                push(
                    out,
                    path,
                    true,
                    format!("values removed: {}", removed.join(", ")),
                );
            }
            if !added.is_empty() {
                push(
                    out,
                    path,
                    false,
                    format!("values added: {}", added.join(", ")),
                );
            }
        }
        (None, Some(_)) => push(out, path, true, "value list added"),
        (Some(_), None) => push(out, path, false, "value list removed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A one-command contract. `args` is the argument schema of `ping`.
    fn contract(args: Value) -> String {
        json!({
            "contract_version": CONTRACT_VERSION,
            "commands": [{
                "name": "ping",
                "kind": "sync",
                "capability": null,
                "args": args,
            }],
        })
        .to_string()
    }

    fn one_struct(fields: Value, required: &[&str]) -> String {
        contract(json!({
            "type": "object",
            "properties": fields,
            "required": required,
            "additionalProperties": false,
        }))
    }

    fn breaking(changes: &[Change]) -> Vec<&str> {
        changes
            .iter()
            .filter(|c| c.breaking)
            .map(|c| c.message.as_str())
            .collect()
    }

    fn name_field(max_len: u64) -> Value {
        json!({ "name": { "type": "string", "maxLength": max_len } })
    }

    #[test]
    fn identical_contracts_have_no_changes() {
        let text = one_struct(name_field(8), &["name"]);
        assert_eq!(diff(&text, &text).unwrap(), Vec::new());
    }

    #[test]
    fn removing_a_command_breaks_and_adding_one_does_not() {
        let old = contract(
            json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
        );
        let empty = json!({ "contract_version": CONTRACT_VERSION, "commands": [] }).to_string();
        let removed = diff(&old, &empty).unwrap();
        assert_eq!(breaking(&removed), vec!["command removed"]);
        let added = diff(&empty, &old).unwrap();
        assert_eq!(added.len(), 1);
        assert!(!added[0].breaking);
    }

    #[test]
    fn capabilities_added_break_and_dropped_ones_do_not() {
        let plain = contract(
            json!({ "type": "object", "properties": {}, "required": [], "additionalProperties": false }),
        );
        let guarded = plain.replace("\"capability\":null", "\"capability\":\"fs.read\"");
        assert_eq!(
            breaking(&diff(&plain, &guarded).unwrap()),
            vec!["now needs capability fs.read"]
        );
        let dropped = diff(&guarded, &plain).unwrap();
        assert_eq!(dropped.len(), 1);
        assert!(!dropped[0].breaking);
    }

    #[test]
    fn a_kind_change_breaks() {
        let text = one_struct(json!({}), &[]);
        let other = text.replace("\"sync\"", "\"job\"");
        assert_eq!(breaking(&diff(&text, &other).unwrap()).len(), 1);
    }

    #[test]
    fn removed_fields_break_and_new_optional_fields_do_not() {
        let old = one_struct(name_field(8), &["name"]);
        let gone = one_struct(json!({}), &[]);
        assert_eq!(breaking(&diff(&old, &gone).unwrap()), vec!["field removed"]);

        let with_optional = one_struct(
            json!({ "name": { "type": "string", "maxLength": 8 }, "note": { "type": "string", "maxLength": 8 } }),
            &["name"],
        );
        let changes = diff(&old, &with_optional).unwrap();
        assert!(breaking(&changes).is_empty());
        assert!(changes.iter().any(|c| c.message == "new optional field"));
    }

    #[test]
    fn a_new_required_field_breaks() {
        let old = one_struct(json!({}), &[]);
        let new = one_struct(
            json!({ "id": { "type": "integer", "minimum": 0, "maximum": 9 } }),
            &["id"],
        );
        assert_eq!(
            breaking(&diff(&old, &new).unwrap()),
            vec!["new required field"]
        );
    }

    #[test]
    fn a_field_becoming_required_breaks_and_the_reverse_does_not() {
        let optional = one_struct(name_field(8), &[]);
        let required = one_struct(name_field(8), &["name"]);
        assert_eq!(
            breaking(&diff(&optional, &required).unwrap()),
            vec!["field is now required"]
        );
        let relaxed = diff(&required, &optional).unwrap();
        assert_eq!(relaxed.len(), 1);
        assert!(!relaxed[0].breaking);
    }

    #[test]
    fn a_lower_length_limit_breaks_and_a_higher_one_does_not() {
        let short = one_struct(name_field(4), &["name"]);
        let long = one_struct(name_field(16), &["name"]);
        assert_eq!(breaking(&diff(&long, &short).unwrap()).len(), 1);
        let loosened = diff(&short, &long).unwrap();
        assert_eq!(loosened.len(), 1);
        assert!(!loosened[0].breaking);
    }

    #[test]
    fn numeric_bounds_break_in_the_tightening_direction_only() {
        let old = one_struct(
            json!({ "n": { "type": "integer", "minimum": 0, "maximum": 10 } }),
            &["n"],
        );
        let tighter_max = one_struct(
            json!({ "n": { "type": "integer", "minimum": 0, "maximum": 5 } }),
            &["n"],
        );
        let raised_min = one_struct(
            json!({ "n": { "type": "integer", "minimum": 1, "maximum": 10 } }),
            &["n"],
        );
        let wider = one_struct(
            json!({ "n": { "type": "integer", "minimum": -3, "maximum": 20 } }),
            &["n"],
        );
        assert_eq!(breaking(&diff(&old, &tighter_max).unwrap()).len(), 1);
        assert_eq!(breaking(&diff(&old, &raised_min).unwrap()).len(), 1);
        assert!(breaking(&diff(&old, &wider).unwrap()).is_empty());
    }

    #[test]
    fn enum_values_removed_break_and_added_ones_do_not() {
        let old = one_struct(
            json!({ "c": { "type": "string", "enum": ["a", "b"] } }),
            &["c"],
        );
        let fewer = one_struct(json!({ "c": { "type": "string", "enum": ["a"] } }), &["c"]);
        let more = one_struct(
            json!({ "c": { "type": "string", "enum": ["a", "b", "c"] } }),
            &["c"],
        );
        assert_eq!(
            breaking(&diff(&old, &fewer).unwrap()),
            vec!["values removed: \"b\""]
        );
        assert!(breaking(&diff(&old, &more).unwrap()).is_empty());
    }

    #[test]
    fn making_a_field_optional_does_not_break() {
        let required = one_struct(name_field(8), &["name"]);
        let optional = one_struct(
            json!({ "name": { "anyOf": [{ "type": "string", "maxLength": 8 }, { "type": "null" }] } }),
            &[],
        );
        let changes = diff(&required, &optional).unwrap();
        assert!(breaking(&changes).is_empty(), "{changes:?}");
        assert!(changes.iter().any(|c| c.message == "now also accepts null"));
    }

    #[test]
    fn a_type_change_breaks() {
        let text = one_struct(name_field(8), &["name"]);
        let number = one_struct(
            json!({ "name": { "type": "integer", "minimum": 0, "maximum": 9 } }),
            &["name"],
        );
        let changes = diff(&text, &number).unwrap();
        assert!(breaking(&changes).contains(&"type changed from string to integer"));
    }

    #[test]
    fn a_pattern_added_breaks() {
        let text = one_struct(name_field(8), &["name"]);
        let patterned = one_struct(
            json!({ "name": { "type": "string", "maxLength": 8, "pattern": "^[a-z]*$" } }),
            &["name"],
        );
        assert_eq!(
            breaking(&diff(&text, &patterned).unwrap()),
            vec!["pattern added"]
        );
    }

    #[test]
    fn a_contract_of_another_version_is_refused() {
        let text = one_struct(name_field(8), &["name"]);
        let other = text.replace(
            &format!("\"contract_version\":{CONTRACT_VERSION}"),
            "\"contract_version\":99",
        );
        assert!(diff(&text, &other).is_err());
        assert!(diff("{", &text).is_err());
    }
}
