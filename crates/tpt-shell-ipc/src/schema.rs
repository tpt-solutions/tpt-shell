// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Argument schemas and the validator that runs before a command handler (spec §5.5, M2).
//!
//! A [`Schema`] describes the `args` object of one command. [`validate`] checks a value
//! against it and reports the first problem with its path, for example `args.items[2].name`.
//! Unknown struct fields are rejected. Absent fields are allowed only where the schema is
//! [`Schema::Optional`].

use serde_json::{Map, Value};

/// Stable code for every argument problem.
pub const INVALID_ARGS: &str = "IPC_INVALID_ARGS";

/// Describes one value the page may send.
#[derive(Clone, Debug, PartialEq)]
pub enum Schema {
    Bool,
    /// Whole numbers in `min..=max`. Fractions are rejected.
    Int {
        min: i64,
        max: i64,
    },
    /// Finite numbers in `min..=max`.
    Float {
        min: f64,
        max: f64,
    },
    /// Strings of at most `max_len` bytes. When `chars` is set, every character must appear in it.
    Str {
        max_len: usize,
        chars: Option<&'static str>,
    },
    /// One of a fixed set of strings.
    Enum(&'static [&'static str]),
    /// The value may also be absent (struct fields) or `null`.
    Optional(&'static Schema),
    /// An array of at most `max_len` items.
    List {
        item: &'static Schema,
        max_len: usize,
    },
    /// An object of string keys, each value matching `value`, with at most `max_len` entries.
    Map {
        value: &'static Schema,
        max_len: usize,
    },
    /// An object with exactly these fields. Any other key is an error.
    Struct(&'static [Field]),
}

/// One field of a [`Schema::Struct`].
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: &'static str,
    pub schema: Schema,
}

/// The first problem found. `path` starts at `args`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// Checks `value` against `schema`. `root` names the value in error paths (usually `"args"`).
pub fn validate(schema: &Schema, value: &Value, root: &str) -> Result<(), ValidationError> {
    check(schema, value, root.to_string())
}

fn fail(path: &str, message: impl Into<String>) -> ValidationError {
    ValidationError {
        path: path.to_string(),
        message: message.into(),
    }
}

fn check(schema: &Schema, value: &Value, path: String) -> Result<(), ValidationError> {
    match schema {
        Schema::Bool => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(fail(&path, "expected a boolean"))
            }
        }
        Schema::Int { min, max } => {
            let n = value
                .as_i64()
                .ok_or_else(|| fail(&path, "expected a whole number"))?;
            if (*min..=*max).contains(&n) {
                Ok(())
            } else {
                Err(fail(&path, format!("{n} is outside {min}..={max}")))
            }
        }
        Schema::Float { min, max } => {
            let n = value
                .as_f64()
                .ok_or_else(|| fail(&path, "expected a number"))?;
            if n.is_finite() && n >= *min && n <= *max {
                Ok(())
            } else {
                Err(fail(&path, format!("{n} is outside {min}..={max}")))
            }
        }
        Schema::Str { max_len, chars } => {
            let s = value
                .as_str()
                .ok_or_else(|| fail(&path, "expected a string"))?;
            if s.len() > *max_len {
                return Err(fail(
                    &path,
                    format!("is {} bytes, limit is {max_len}", s.len()),
                ));
            }
            if let Some(allowed) = chars {
                if let Some(bad) = s.chars().find(|c| !allowed.contains(*c)) {
                    return Err(fail(&path, format!("character {bad:?} is not allowed")));
                }
            }
            Ok(())
        }
        Schema::Enum(options) => {
            let s = value
                .as_str()
                .ok_or_else(|| fail(&path, "expected a string"))?;
            if options.contains(&s) {
                Ok(())
            } else {
                Err(fail(&path, format!("`{s}` is not one of {options:?}")))
            }
        }
        Schema::Optional(inner) => {
            if value.is_null() {
                Ok(())
            } else {
                check(inner, value, path)
            }
        }
        Schema::List { item, max_len } => {
            let items = value
                .as_array()
                .ok_or_else(|| fail(&path, "expected an array"))?;
            if items.len() > *max_len {
                return Err(fail(
                    &path,
                    format!("has {} items, limit is {max_len}", items.len()),
                ));
            }
            for (i, v) in items.iter().enumerate() {
                check(item, v, format!("{path}[{i}]"))?;
            }
            Ok(())
        }
        Schema::Map {
            value: inner,
            max_len,
        } => {
            let map = value
                .as_object()
                .ok_or_else(|| fail(&path, "expected an object"))?;
            if map.len() > *max_len {
                return Err(fail(
                    &path,
                    format!("has {} entries, limit is {max_len}", map.len()),
                ));
            }
            for (k, v) in map {
                check(inner, v, format!("{path}.{k}"))?;
            }
            Ok(())
        }
        Schema::Struct(fields) => {
            let map = value
                .as_object()
                .ok_or_else(|| fail(&path, "expected an object"))?;
            check_struct(fields, map, &path)
        }
    }
}

fn check_struct(
    fields: &[Field],
    map: &Map<String, Value>,
    path: &str,
) -> Result<(), ValidationError> {
    if let Some(extra) = map
        .keys()
        .find(|k| !fields.iter().any(|f| f.name == k.as_str()))
    {
        return Err(fail(path, format!("unknown field `{extra}`")));
    }
    for field in fields {
        let child = format!("{path}.{}", field.name);
        match map.get(field.name) {
            Some(v) => check(&field.schema, v, child)?,
            None => {
                if !matches!(field.schema, Schema::Optional(_)) {
                    return Err(fail(&child, "is required"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FILE_FIELDS: &[Field] = &[
        Field {
            name: "path",
            schema: Schema::Str {
                max_len: 260,
                chars: None,
            },
        },
        Field {
            name: "mode",
            schema: Schema::Enum(&["read", "write"]),
        },
        Field {
            name: "retries",
            schema: Schema::Optional(&Schema::Int { min: 0, max: 5 }),
        },
    ];

    fn file_schema() -> Schema {
        Schema::Struct(FILE_FIELDS)
    }

    #[test]
    fn accepts_a_valid_struct() {
        let v = json!({ "path": "C:/a.txt", "mode": "read", "retries": 2 });
        assert_eq!(validate(&file_schema(), &v, "args"), Ok(()));
    }

    #[test]
    fn optional_field_may_be_absent_or_null() {
        let v = json!({ "path": "a", "mode": "write" });
        assert_eq!(validate(&file_schema(), &v, "args"), Ok(()));
        let v = json!({ "path": "a", "mode": "write", "retries": null });
        assert_eq!(validate(&file_schema(), &v, "args"), Ok(()));
    }

    #[test]
    fn missing_required_field_is_named() {
        let v = json!({ "mode": "read" });
        let e = validate(&file_schema(), &v, "args").unwrap_err();
        assert_eq!(e.path, "args.path");
        assert_eq!(e.message, "is required");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let v = json!({ "path": "a", "mode": "read", "admin": true });
        let e = validate(&file_schema(), &v, "args").unwrap_err();
        assert_eq!(e.path, "args");
        assert!(e.message.contains("`admin`"));
    }

    #[test]
    fn wrong_type_is_reported_with_path() {
        let v = json!({ "path": 5, "mode": "read" });
        let e = validate(&file_schema(), &v, "args").unwrap_err();
        assert_eq!(e.path, "args.path");
        assert_eq!(e.message, "expected a string");
    }

    #[test]
    fn enum_values_are_checked() {
        let v = json!({ "path": "a", "mode": "delete" });
        assert!(validate(&file_schema(), &v, "args").is_err());
    }

    #[test]
    fn integer_bounds_and_fractions_are_checked() {
        let too_big = json!({ "path": "a", "mode": "read", "retries": 6 });
        assert!(validate(&file_schema(), &too_big, "args").is_err());
        let fraction = json!({ "path": "a", "mode": "read", "retries": 1.5 });
        let e = validate(&file_schema(), &fraction, "args").unwrap_err();
        assert_eq!(e.message, "expected a whole number");
    }

    #[test]
    fn string_length_counts_bytes() {
        let schema = Schema::Str {
            max_len: 3,
            chars: None,
        };
        assert!(validate(&schema, &json!("abc"), "s").is_ok());
        assert!(validate(&schema, &json!("abcd"), "s").is_err());
        // "é" is two bytes: two of them is four bytes, over the limit.
        assert!(validate(&schema, &json!("éé"), "s").is_err());
    }

    #[test]
    fn allowed_characters_are_enforced() {
        let schema = Schema::Str {
            max_len: 10,
            chars: Some("abcdefghijklmnopqrstuvwxyz-"),
        };
        assert!(validate(&schema, &json!("my-app"), "s").is_ok());
        let e = validate(&schema, &json!("my app"), "s").unwrap_err();
        assert!(e.message.contains("' '"));
    }

    #[test]
    fn lists_report_the_bad_index() {
        let schema = Schema::List {
            item: &Schema::Int { min: 0, max: 9 },
            max_len: 3,
        };
        assert!(validate(&schema, &json!([1, 2, 3]), "xs").is_ok());
        let e = validate(&schema, &json!([1, 2, 30]), "xs").unwrap_err();
        assert_eq!(e.path, "xs[2]");
        assert!(validate(&schema, &json!([1, 2, 3, 4]), "xs").is_err());
    }

    #[test]
    fn maps_check_every_value_and_size() {
        let schema = Schema::Map {
            value: &Schema::Bool,
            max_len: 2,
        };
        assert!(validate(&schema, &json!({ "a": true, "b": false }), "m").is_ok());
        let e = validate(&schema, &json!({ "a": true, "b": 1 }), "m").unwrap_err();
        assert_eq!(e.path, "m.b");
        assert!(validate(&schema, &json!({ "a": true, "b": true, "c": true }), "m").is_err());
    }

    #[test]
    fn nested_structs_build_full_paths() {
        const INNER: &[Field] = &[Field {
            name: "size",
            schema: Schema::Float {
                min: 0.0,
                max: 10.0,
            },
        }];
        const OUTER: &[Field] = &[Field {
            name: "window",
            schema: Schema::Struct(INNER),
        }];
        let e = validate(
            &Schema::Struct(OUTER),
            &json!({ "window": { "size": 11.0 } }),
            "args",
        )
        .unwrap_err();
        assert_eq!(e.path, "args.window.size");
    }
}
