// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! IPC envelope v1 (spec §5.5): parsing with limits, and response and event builders.
//!
//! JS -> Rust: `{"v":1,"id":<u64>,"cmd":"name","args":{...}}`
//! Rust -> JS: `{"id":..,"ok":true,"data":..}` or `{"id":..,"ok":false,"error":{code,message}}`
//! Events:     `{"event":"name","data":..}`
//!
//! Unknown envelope versions are rejected. Unknown envelope fields are rejected.

use serde_json::{json, Map};

pub use executor::block_on;
pub use router::{encode, Caller, CommandError, Router, Target};
pub use scoped::{DeviceId, ScopeError, ScopedPath};
pub use serde_json::Value;

pub mod blob;
pub mod contract;
pub mod contract_diff;
pub mod executor;
pub mod jobs;
pub mod rate_limit;
pub mod reflect;
pub mod router;
pub mod schema;
pub mod scoped;
pub mod stream;
pub mod typescript;

/// The only envelope version this crate understands.
pub const VERSION: u64 = 1;

/// Input limits, all configurable (spec §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted message, in bytes.
    pub max_message_bytes: usize,
    /// Deepest accepted JSON nesting.
    pub max_depth: usize,
    /// Longest accepted command name, in bytes.
    pub max_cmd_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_message_bytes: 1024 * 1024,
            max_depth: 32,
            max_cmd_len: 128,
        }
    }
}

/// A parsed, shape-checked request. Argument validation against the contract is a later step.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: u64,
    pub cmd: String,
    pub args: Value,
}

/// Why a message was refused. Each variant has a stable code (see [`ParseError::code`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    TooLarge { size: usize, max: usize },
    Malformed(String),
    NotAnObject,
    UnsupportedVersion(u64),
    MissingField(&'static str),
    WrongType(&'static str),
    UnknownField(String),
    EmptyCommand,
    CommandTooLong { len: usize, max: usize },
    TooDeep { max: usize },
}

impl ParseError {
    /// Stable machine-readable code, sent to the page in error responses.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::TooLarge { .. } => "IPC_TOO_LARGE",
            ParseError::Malformed(_) => "IPC_MALFORMED",
            ParseError::NotAnObject => "IPC_NOT_OBJECT",
            ParseError::UnsupportedVersion(_) => "IPC_UNSUPPORTED_VERSION",
            ParseError::MissingField(_) => "IPC_MISSING_FIELD",
            ParseError::WrongType(_) => "IPC_WRONG_TYPE",
            ParseError::UnknownField(_) => "IPC_UNKNOWN_FIELD",
            ParseError::EmptyCommand => "IPC_EMPTY_COMMAND",
            ParseError::CommandTooLong { .. } => "IPC_COMMAND_TOO_LONG",
            ParseError::TooDeep { .. } => "IPC_TOO_DEEP",
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::TooLarge { size, max } => {
                write!(f, "message is {size} bytes, limit is {max}")
            }
            ParseError::Malformed(e) => write!(f, "message is not valid JSON: {e}"),
            ParseError::NotAnObject => write!(f, "message must be a JSON object"),
            ParseError::UnsupportedVersion(v) => write!(
                f,
                "envelope version {v} is not supported; expected {VERSION}"
            ),
            ParseError::MissingField(name) => write!(f, "missing field `{name}`"),
            ParseError::WrongType(name) => write!(f, "field `{name}` has the wrong type"),
            ParseError::UnknownField(name) => write!(f, "unknown field `{name}`"),
            ParseError::EmptyCommand => write!(f, "command name is empty"),
            ParseError::CommandTooLong { len, max } => {
                write!(f, "command name is {len} bytes, limit is {max}")
            }
            ParseError::TooDeep { max } => write!(f, "JSON nesting exceeds {max}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses and shape-checks one JS -> Rust message.
pub fn parse_request(bytes: &[u8], limits: &Limits) -> Result<Request, ParseError> {
    if bytes.len() > limits.max_message_bytes {
        return Err(ParseError::TooLarge {
            size: bytes.len(),
            max: limits.max_message_bytes,
        });
    }

    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| ParseError::Malformed(e.to_string()))?;
    let obj = value.as_object().ok_or(ParseError::NotAnObject)?;

    if let Some(extra) = obj
        .keys()
        .find(|k| !matches!(k.as_str(), "v" | "id" | "cmd" | "args"))
    {
        return Err(ParseError::UnknownField(extra.clone()));
    }

    let version = obj
        .get("v")
        .ok_or(ParseError::MissingField("v"))?
        .as_u64()
        .ok_or(ParseError::WrongType("v"))?;
    if version != VERSION {
        return Err(ParseError::UnsupportedVersion(version));
    }

    let id = obj
        .get("id")
        .ok_or(ParseError::MissingField("id"))?
        .as_u64()
        .ok_or(ParseError::WrongType("id"))?;

    let cmd = obj
        .get("cmd")
        .ok_or(ParseError::MissingField("cmd"))?
        .as_str()
        .ok_or(ParseError::WrongType("cmd"))?;
    if cmd.is_empty() {
        return Err(ParseError::EmptyCommand);
    }
    if cmd.len() > limits.max_cmd_len {
        return Err(ParseError::CommandTooLong {
            len: cmd.len(),
            max: limits.max_cmd_len,
        });
    }

    let args = match obj.get("args") {
        None => Value::Object(Map::new()),
        Some(v @ Value::Object(_)) => v.clone(),
        Some(_) => return Err(ParseError::WrongType("args")),
    };

    if depth(&value) > limits.max_depth {
        return Err(ParseError::TooDeep {
            max: limits.max_depth,
        });
    }

    Ok(Request {
        id,
        cmd: cmd.to_string(),
        args,
    })
}

/// Nesting depth: a scalar is 0, a container is 1 plus its deepest child.
fn depth(v: &Value) -> usize {
    match v {
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

/// Success response for request `id`.
pub fn ok_response(id: u64, data: Value) -> Value {
    json!({ "id": id, "ok": true, "data": data })
}

/// Error response for request `id`.
pub fn err_response(id: u64, code: &str, message: &str) -> Value {
    json!({ "id": id, "ok": false, "error": { "code": code, "message": message } })
}

/// Error response for a message that could not be parsed. There is no id, so it is `null`.
pub fn parse_error_response(err: &ParseError) -> Value {
    json!({ "id": null, "ok": false, "error": { "code": err.code(), "message": err.to_string() } })
}

/// Event sent from Rust to the page.
pub fn event(name: &str, data: Value) -> Value {
    json!({ "event": name, "data": data })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Request, ParseError> {
        parse_request(s.as_bytes(), &Limits::default())
    }

    #[test]
    fn parses_a_valid_request() {
        let r = parse(r#"{"v":1,"id":7,"cmd":"list_dir","args":{"path":"C:/"}}"#).unwrap();
        assert_eq!(r.id, 7);
        assert_eq!(r.cmd, "list_dir");
        assert_eq!(r.args, json!({ "path": "C:/" }));
    }

    #[test]
    fn missing_args_default_to_empty_object() {
        let r = parse(r#"{"v":1,"id":1,"cmd":"ping"}"#).unwrap();
        assert_eq!(r.args, json!({}));
    }

    #[test]
    fn rejects_oversized_message_before_parsing() {
        let limits = Limits {
            max_message_bytes: 10,
            ..Limits::default()
        };
        let err = parse_request(b"{\"v\":1,\"id\":1,\"cmd\":\"x\"}", &limits).unwrap_err();
        assert!(matches!(err, ParseError::TooLarge { max: 10, .. }));
    }

    #[test]
    fn rejects_invalid_json() {
        assert_eq!(parse("{nope").unwrap_err().code(), "IPC_MALFORMED");
    }

    #[test]
    fn rejects_non_object() {
        assert_eq!(parse("[1,2]").unwrap_err(), ParseError::NotAnObject);
    }

    #[test]
    fn rejects_other_versions() {
        assert_eq!(
            parse(r#"{"v":2,"id":1,"cmd":"x"}"#).unwrap_err(),
            ParseError::UnsupportedVersion(2)
        );
    }

    #[test]
    fn rejects_missing_version() {
        assert_eq!(
            parse(r#"{"id":1,"cmd":"x"}"#).unwrap_err(),
            ParseError::MissingField("v")
        );
    }

    #[test]
    fn rejects_unknown_envelope_fields() {
        assert_eq!(
            parse(r#"{"v":1,"id":1,"cmd":"x","admin":true}"#).unwrap_err(),
            ParseError::UnknownField("admin".into())
        );
    }

    #[test]
    fn rejects_wrong_types() {
        assert_eq!(
            parse(r#"{"v":1,"id":"1","cmd":"x"}"#).unwrap_err(),
            ParseError::WrongType("id")
        );
        assert_eq!(
            parse(r#"{"v":1,"id":1,"cmd":5}"#).unwrap_err(),
            ParseError::WrongType("cmd")
        );
        assert_eq!(
            parse(r#"{"v":1,"id":1,"cmd":"x","args":[]}"#).unwrap_err(),
            ParseError::WrongType("args")
        );
        assert_eq!(
            parse(r#"{"v":1,"id":-1,"cmd":"x"}"#).unwrap_err(),
            ParseError::WrongType("id")
        );
    }

    #[test]
    fn rejects_empty_and_long_commands() {
        assert_eq!(
            parse(r#"{"v":1,"id":1,"cmd":""}"#).unwrap_err(),
            ParseError::EmptyCommand
        );
        let long = "a".repeat(129);
        let msg = format!(r#"{{"v":1,"id":1,"cmd":"{long}"}}"#);
        assert!(matches!(
            parse(&msg).unwrap_err(),
            ParseError::CommandTooLong { len: 129, max: 128 }
        ));
    }

    #[test]
    fn rejects_deep_nesting() {
        let open = "[".repeat(40);
        let close = "]".repeat(40);
        let msg = format!(r#"{{"v":1,"id":1,"cmd":"x","args":{{"a":{open}{close}}}}}"#);
        assert!(matches!(
            parse(&msg).unwrap_err(),
            ParseError::TooDeep { max: 32 }
        ));
    }

    #[test]
    fn accepts_nesting_at_the_limit() {
        // args object (1) + array (1) + array (1) = depth 3 for the args value; whole envelope = 4.
        let r = parse(r#"{"v":1,"id":1,"cmd":"x","args":{"a":[[1]]}}"#);
        assert!(r.is_ok());
    }

    #[test]
    fn responses_have_the_documented_shape() {
        assert_eq!(
            ok_response(3, json!(42)),
            json!({ "id": 3, "ok": true, "data": 42 })
        );
        assert_eq!(
            err_response(3, "X", "bad"),
            json!({ "id": 3, "ok": false, "error": { "code": "X", "message": "bad" } })
        );
        assert_eq!(
            event("tick", json!(1)),
            json!({ "event": "tick", "data": 1 })
        );
    }

    #[test]
    fn parse_error_response_has_null_id() {
        let v = parse_error_response(&ParseError::NotAnObject);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], "IPC_NOT_OBJECT");
    }
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn rng(seed: u64) -> impl FnMut() -> u64 {
        let mut state = seed;
        move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        }
    }

    #[test]
    fn random_bytes_never_panic_the_parser() {
        let mut next = rng(0x2545_F491_4F6C_DD1D);
        let limits = Limits::default();
        for _ in 0..50_000 {
            let len = (next() % 200) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = parse_request(&bytes, &limits);
        }
    }

    #[test]
    fn mutated_valid_envelopes_never_panic_the_parser() {
        let base = br#"{"v":1,"id":7,"cmd":"list_dir","args":{"path":"C:/a","n":[1,[2,[3]]]}}"#;
        let mut next = rng(0x8CB9_2BA7_2F3D_8DD7);
        let limits = Limits::default();
        for _ in 0..50_000 {
            let mut bytes = base.to_vec();
            for _ in 0..1 + (next() % 4) {
                let at = (next() as usize) % bytes.len();
                let alphabet: &[u8] = b"{}[]\":,0-x \\";
                bytes[at] = alphabet[(next() as usize) % alphabet.len()];
            }
            let _ = parse_request(&bytes, &limits);
        }
    }
}
