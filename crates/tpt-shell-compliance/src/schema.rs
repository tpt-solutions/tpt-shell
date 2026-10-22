// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A small JSON Schema checker for the SBOM tests. It covers the keywords the CycloneDX 1.5 and
//! SPDX 2.3 schemas use for the fields tpt-shell writes: `type`, `required`, `properties`,
//! `additionalProperties: false`, `items`, `enum`, `const`, `minItems` and `minLength`.
//!
//! It does not follow `$ref`, `oneOf` or `anyOf`, so a subschema behind one of those is not
//! checked. The full validation is the ajv run in `tools/validate-sbom.sh`. This checker
//! exists so that the required fields and types are checked in `cargo test`, with no network.

use serde_json::Value;

/// Checks `instance` against `schema`. Returns one line per problem, with the JSON path of
/// the value. An empty list means the instance passes the keywords this checker knows.
///
/// Support: Partial. It does not follow $ref, oneOf or anyOf, so a subschema behind one of those is
/// not checked here; the full validation is the ajv run in tools/validate-sbom.sh.
pub fn check(instance: &Value, schema: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    walk(instance, schema, "$", &mut errors);
    errors
}

fn type_matches(instance: &Value, name: &str) -> bool {
    match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "number" => instance.is_number(),
        "integer" => {
            instance.is_i64()
                || instance.is_u64()
                || instance.as_f64().is_some_and(|f| f.fract() == 0.0)
        }
        _ => true,
    }
}

fn walk(instance: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    if let Some(expected) = schema.get("type") {
        let names: Vec<&str> = match expected {
            Value::String(s) => vec![s.as_str()],
            Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !names.is_empty() && !names.iter().any(|n| type_matches(instance, n)) {
            errors.push(format!("{path}: expected {}", names.join(" or ")));
            return;
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(instance) {
            errors.push(format!("{path}: value is not one of the allowed values"));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != instance {
            errors.push(format!("{path}: value must be {constant}"));
        }
    }
    if let Some(text) = instance.as_str() {
        if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
            if (text.chars().count() as u64) < min {
                errors.push(format!("{path}: shorter than {min} characters"));
            }
        }
    }
    if let Some(items) = instance.as_array() {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
            if (items.len() as u64) < min {
                errors.push(format!("{path}: fewer than {min} items"));
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for (i, item) in items.iter().enumerate() {
                walk(item, item_schema, &format!("{path}[{i}]"), errors);
            }
        }
    }
    if let Some(object) = instance.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    errors.push(format!("{path}: missing required `{key}`"));
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(properties) = properties {
            for (key, sub) in properties {
                if let Some(value) = object.get(key) {
                    walk(value, sub, &format!("{path}.{key}"), errors);
                }
            }
        }
        if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
            for key in object.keys() {
                if !properties.is_some_and(|p| p.contains_key(key)) {
                    errors.push(format!("{path}: unexpected property `{key}`"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "required": ["bomFormat", "components"],
            "additionalProperties": false,
            "properties": {
                "bomFormat": { "const": "CycloneDX" },
                "version": { "type": "integer" },
                "components": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["name"],
                        "properties": { "name": { "type": "string", "minLength": 1 } }
                    }
                }
            }
        })
    }

    #[test]
    fn a_valid_document_passes() {
        let doc =
            json!({ "bomFormat": "CycloneDX", "version": 1, "components": [{ "name": "a" }] });
        assert!(check(&doc, &schema()).is_empty());
    }

    #[test]
    fn missing_fields_wrong_types_and_extra_keys_are_reported_with_paths() {
        let doc = json!({
            "bomFormat": "SPDX",
            "version": "one",
            "components": [{ "name": "" }, {}],
            "extra": true
        });
        let errors = check(&doc, &schema());
        assert!(errors
            .iter()
            .any(|e| e.starts_with("$.bomFormat: value must be")));
        assert!(errors
            .iter()
            .any(|e| e.starts_with("$.version: expected integer")));
        assert!(errors
            .iter()
            .any(|e| e.starts_with("$.components[0].name: shorter")));
        assert!(errors
            .iter()
            .any(|e| e.starts_with("$.components[1]: missing required `name`")));
        assert!(errors
            .iter()
            .any(|e| e.contains("unexpected property `extra`")));
    }

    #[test]
    fn a_missing_top_level_field_is_reported() {
        let errors = check(&json!({ "bomFormat": "CycloneDX" }), &schema());
        assert_eq!(errors, vec!["$: missing required `components`".to_string()]);
    }

    #[test]
    fn enums_and_min_items_are_checked() {
        let s = json!({ "enum": ["a", "b"], "minItems": 1 });
        assert!(check(&json!("a"), &s).is_empty());
        assert_eq!(check(&json!("c"), &s).len(), 1);
        assert_eq!(check(&json!([]), &json!({ "minItems": 1 })).len(), 1);
    }
}
