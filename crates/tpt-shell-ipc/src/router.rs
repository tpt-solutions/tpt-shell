// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Routes parsed requests to registered handlers and builds the response text (spec §5.5, §8).
//! Backend-independent: a platform backend passes each page message through [`Router::handle`]
//! and posts the returned text back to the page.

use std::collections::HashMap;

use crate::{
    err_response, ok_response, parse_error_response, parse_request, Limits, Request, Value,
};

/// A failed command. `code` is stable and meant for the page; `message` is for developers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl CommandError {
    pub fn new(code: &str, message: &str) -> Self {
        CommandError {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

type Handler = Box<dyn Fn(&Value) -> Result<Value, CommandError>>;

/// Maps command names to handlers. Each handler receives the request's `args` object.
#[derive(Default)]
pub struct Router {
    handlers: HashMap<String, Handler>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `handler` for `cmd`. A later registration for the same name replaces the earlier one.
    pub fn register(
        &mut self,
        cmd: &str,
        handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
    ) {
        self.handlers.insert(cmd.to_string(), Box::new(handler));
    }

    /// Handles one raw message from the page and returns the response text to send back.
    /// Malformed messages get an error response with a null id; they are never dispatched.
    pub fn handle(&self, bytes: &[u8], limits: &Limits) -> String {
        let value = match parse_request(bytes, limits) {
            Ok(request) => self.dispatch(&request),
            Err(err) => parse_error_response(&err),
        };
        encode(&value)
    }

    fn dispatch(&self, request: &Request) -> Value {
        match self.handlers.get(&request.cmd) {
            None => err_response(
                request.id,
                "IPC_UNKNOWN_COMMAND",
                &format!("no command named `{}`", request.cmd),
            ),
            Some(handler) => match handler(&request.args) {
                Ok(data) => ok_response(request.id, data),
                Err(err) => err_response(request.id, &err.code, &err.message),
            },
        }
    }
}

/// Serializes a response value to the text sent over the wire.
pub fn encode(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| r#"{"id":null,"ok":false}"#.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn router() -> Router {
        let mut r = Router::new();
        r.register("echo", |args| Ok(args.clone()));
        r.register("fail", |_| {
            Err(CommandError::new("APP_DENIED", "not today"))
        });
        r
    }

    fn reply(r: &Router, msg: &str) -> Value {
        serde_json::from_str(&r.handle(msg.as_bytes(), &Limits::default())).unwrap()
    }

    #[test]
    fn dispatches_to_the_registered_handler() {
        let v = reply(
            &router(),
            r#"{"v":1,"id":4,"cmd":"echo","args":{"msg":"hi"}}"#,
        );
        assert_eq!(v, json!({ "id": 4, "ok": true, "data": { "msg": "hi" } }));
    }

    #[test]
    fn handler_errors_keep_their_code() {
        let v = reply(&router(), r#"{"v":1,"id":5,"cmd":"fail"}"#);
        assert_eq!(
            v,
            json!({ "id": 5, "ok": false, "error": { "code": "APP_DENIED", "message": "not today" } })
        );
    }

    #[test]
    fn unknown_commands_are_reported_not_dropped() {
        let v = reply(&router(), r#"{"v":1,"id":6,"cmd":"nope"}"#);
        assert_eq!(v["id"], 6);
        assert_eq!(v["error"]["code"], "IPC_UNKNOWN_COMMAND");
    }

    #[test]
    fn malformed_messages_get_a_null_id_error() {
        let v = reply(&router(), r#"{"v":2,"id":1,"cmd":"echo"}"#);
        assert_eq!(v["id"], Value::Null);
        assert_eq!(v["error"]["code"], "IPC_UNSUPPORTED_VERSION");
    }

    #[test]
    fn later_registration_replaces_earlier() {
        let mut r = Router::new();
        r.register("x", |_| Ok(json!(1)));
        r.register("x", |_| Ok(json!(2)));
        assert_eq!(reply(&r, r#"{"v":1,"id":1,"cmd":"x"}"#)["data"], 2);
    }
}
