// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Maps Rust argument types to [`Schema`]s, for the `#[command]` macro (spec M2).
//!
//! Schemas hold `&'static` references, so building one for a nested type leaks a small box.
//! That happens once per command, when the contract is built at startup.

use std::collections::HashMap;

use crate::schema::Schema;

/// Types that can be an argument of a command.
pub trait ToSchema {
    fn schema() -> Schema;
}

/// Longest string argument accepted by default (bytes).
pub const DEFAULT_MAX_STRING: usize = 64 * 1024;
/// Longest list or map argument accepted by default (items).
pub const DEFAULT_MAX_ITEMS: usize = 4096;

impl ToSchema for bool {
    fn schema() -> Schema {
        Schema::Bool
    }
}

macro_rules! int_schema {
    ($($t:ty),*) => {$(
        impl ToSchema for $t {
            fn schema() -> Schema {
                Schema::Int { min: <$t>::MIN as i64, max: <$t>::MAX as i64 }
            }
        }
    )*};
}
// Only types whose range fits in i64, because the validator reads whole numbers as i64.
int_schema!(i8, i16, i32, i64, u8, u16, u32);

impl ToSchema for f64 {
    fn schema() -> Schema {
        Schema::Float {
            min: f64::MIN,
            max: f64::MAX,
        }
    }
}

impl ToSchema for String {
    fn schema() -> Schema {
        Schema::Str {
            max_len: DEFAULT_MAX_STRING,
            chars: None,
        }
    }
}

impl<T: ToSchema> ToSchema for Vec<T> {
    fn schema() -> Schema {
        Schema::List {
            item: leak(T::schema()),
            max_len: DEFAULT_MAX_ITEMS,
        }
    }
}

impl<T: ToSchema> ToSchema for Option<T> {
    fn schema() -> Schema {
        Schema::Optional(leak(T::schema()))
    }
}

impl<T: ToSchema> ToSchema for HashMap<String, T> {
    fn schema() -> Schema {
        Schema::Map {
            value: leak(T::schema()),
            max_len: DEFAULT_MAX_ITEMS,
        }
    }
}

fn leak(schema: Schema) -> &'static Schema {
    Box::leak(Box::new(schema))
}

/// Support for generated code. Not part of the public API.
#[doc(hidden)]
pub mod __private {
    use serde::de::DeserializeOwned;
    use serde_json::Value;

    use crate::router::CommandError;
    use crate::schema::{Field, Schema};

    pub use serde_json;

    /// Leaks a struct's fields so the schema can hold them.
    pub fn struct_schema(fields: Vec<Field>) -> Schema {
        Schema::Struct(Box::leak(fields.into_boxed_slice()))
    }

    /// Reads one argument. A missing argument is read as `null`, which `Option` accepts.
    pub fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, CommandError> {
        let value = args.get(name).cloned().unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(|e| {
            CommandError::new(crate::schema::INVALID_ARGS, &format!("args.{name}: {e}"))
        })
    }

    /// Bounds a `#[derive(Contract)]` field may set. Each one applies only to the schema kind it
    /// belongs to (see [`bounded`]).
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Bounds {
        pub min: Option<i64>,
        pub max: Option<i64>,
        pub max_len: Option<usize>,
        pub max_items: Option<usize>,
        pub chars: Option<&'static str>,
    }

    /// Applies a field's bounds to the schema of its type. Panics when a bound does not fit the
    /// type (`max_len` on a number, for example): the schema is built at start-up, so the
    /// mistake shows at once instead of silently dropping the limit.
    pub fn bounded(schema: Schema, b: Bounds) -> Schema {
        fn misplaced(name: &str) -> ! {
            panic!("contract bound `{name}` does not apply to this field's type")
        }
        fn check(name: &str, set: bool) {
            if set {
                misplaced(name);
            }
        }
        match schema {
            Schema::Optional(inner) => {
                Schema::Optional(Box::leak(Box::new(bounded((*inner).clone(), b))))
            }
            Schema::Int { min, max } => {
                check("max_len", b.max_len.is_some());
                check("max_items", b.max_items.is_some());
                check("chars", b.chars.is_some());
                Schema::Int {
                    min: b.min.unwrap_or(min),
                    max: b.max.unwrap_or(max),
                }
            }
            Schema::Float { min, max } => {
                check("max_len", b.max_len.is_some());
                check("max_items", b.max_items.is_some());
                check("chars", b.chars.is_some());
                Schema::Float {
                    min: b.min.map_or(min, |v| v as f64),
                    max: b.max.map_or(max, |v| v as f64),
                }
            }
            Schema::Str { max_len, chars } => {
                check("min", b.min.is_some());
                check("max", b.max.is_some());
                check("max_items", b.max_items.is_some());
                Schema::Str {
                    max_len: b.max_len.unwrap_or(max_len),
                    chars: b.chars.or(chars),
                }
            }
            Schema::List { item, max_len } => {
                check("min", b.min.is_some());
                check("max", b.max.is_some());
                check("max_len", b.max_len.is_some());
                check("chars", b.chars.is_some());
                Schema::List {
                    item,
                    max_len: b.max_items.unwrap_or(max_len),
                }
            }
            Schema::Map { value, max_len } => {
                check("min", b.min.is_some());
                check("max", b.max.is_some());
                check("max_len", b.max_len.is_some());
                check("chars", b.chars.is_some());
                Schema::Map {
                    value,
                    max_len: b.max_items.unwrap_or(max_len),
                }
            }
            other => {
                check("min", b.min.is_some());
                check("max", b.max.is_some());
                check("max_len", b.max_len.is_some());
                check("max_items", b.max_items.is_some());
                check("chars", b.chars.is_some());
                other
            }
        }
    }

    /// Serializes a handler's result.
    pub fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, CommandError> {
        serde_json::to_value(value).map_err(|e| CommandError::new("IPC_BAD_RESULT", &e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;

    #[test]
    fn integer_types_get_their_full_range() {
        assert_eq!(u8::schema(), Schema::Int { min: 0, max: 255 });
        assert_eq!(
            i32::schema(),
            Schema::Int {
                min: i32::MIN as i64,
                max: i32::MAX as i64
            }
        );
    }

    #[test]
    fn containers_nest_their_element_schema() {
        match Vec::<bool>::schema() {
            Schema::List { item, max_len } => {
                assert_eq!(*item, Schema::Bool);
                assert_eq!(max_len, DEFAULT_MAX_ITEMS);
            }
            other => panic!("expected a list, got {other:?}"),
        }
        assert!(matches!(Option::<String>::schema(), Schema::Optional(_)));
    }

    #[test]
    fn missing_arguments_read_as_none_and_required_ones_fail() {
        let args = serde_json::json!({ "a": 1 });
        let present: i64 = __private::arg(&args, "a").unwrap();
        assert_eq!(present, 1);
        let absent: Option<i64> = __private::arg(&args, "b").unwrap();
        assert_eq!(absent, None);
        assert!(__private::arg::<i64>(&args, "b").is_err());
    }

    #[test]
    fn struct_schema_keeps_field_order() {
        let schema = __private::struct_schema(vec![
            Field {
                name: "x",
                schema: Schema::Bool,
            },
            Field {
                name: "y",
                schema: Schema::Bool,
            },
        ]);
        match schema {
            Schema::Struct(fields) => {
                let names: Vec<&str> = fields.iter().map(|f| f.name).collect();
                assert_eq!(names, vec!["x", "y"]);
            }
            other => panic!("expected a struct, got {other:?}"),
        }
    }
}
