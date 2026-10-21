// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Routes parsed requests to registered handlers and builds the response text (spec §5.5, §8).
//! Backend-independent: a platform backend passes each page message through [`Router::handle`]
//! and posts the returned text back to the page.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use crate::rate_limit::RateLimiter;
use crate::schema::{self, validate, Schema};
use crate::{
    err_response, ok_response, parse_error_response, parse_request, Limits, Request, Value,
};

/// A failed command. `code` is stable and meant for the page; `message` is for developers.
/// `details` is optional structured data for the page, such as the path of a bad argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

impl CommandError {
    pub fn new(code: &str, message: &str) -> Self {
        CommandError {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
        }
    }

    /// Attaches structured details, sent as `error.details` in the response.
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
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

    /// Registers `handler` for `cmd`, guarded by `schema`. The arguments are checked first; a
    /// bad value is answered with `IPC_INVALID_ARGS` and the handler never runs.
    pub fn register_validated(
        &mut self,
        cmd: &str,
        schema: Schema,
        handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
    ) {
        self.register(cmd, move |args| {
            validate(&schema, args, "args").map_err(|e| {
                CommandError::new(schema::INVALID_ARGS, &e.to_string())
                    .with_details(json!({ "path": e.path }))
            })?;
            handler(args)
        });
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

    /// Like [`Router::handle`], but a well-formed request is refused with `IPC_RATE_LIMITED`
    /// when `limiter` has no token left at `now`. Malformed messages do not use a token: the
    /// size cap already bounds the cost of rejecting them.
    pub fn handle_limited(
        &self,
        bytes: &[u8],
        limits: &Limits,
        limiter: &mut RateLimiter,
        now: Duration,
    ) -> String {
        let value = match parse_request(bytes, limits) {
            Ok(request) if limiter.allow(now) => self.dispatch(&request),
            Ok(request) => err_response(
                request.id,
                "IPC_RATE_LIMITED",
                "too many commands from this window; slow down and retry",
            ),
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
                Err(err) => {
                    let mut response = err_response(request.id, &err.code, &err.message);
                    if let Some(details) = err.details {
                        response["error"]["details"] = details;
                    }
                    response
                }
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
    fn rate_limited_requests_get_an_error_with_their_id() {
        use crate::rate_limit::{RateLimit, RateLimiter};
        let mut limiter = RateLimiter::new(RateLimit {
            burst: 1,
            per_second: 1,
        });
        let r = router();
        let msg = br#"{"v":1,"id":9,"cmd":"echo","args":{}}"#;
        let limits = Limits::default();
        let now = Duration::from_secs(1);

        let first: Value =
            serde_json::from_str(&r.handle_limited(msg, &limits, &mut limiter, now)).unwrap();
        assert_eq!(first["ok"], true);

        let second: Value =
            serde_json::from_str(&r.handle_limited(msg, &limits, &mut limiter, now)).unwrap();
        assert_eq!(second["id"], 9);
        assert_eq!(second["error"]["code"], "IPC_RATE_LIMITED");
    }

    #[test]
    fn malformed_messages_do_not_spend_tokens() {
        use crate::rate_limit::{RateLimit, RateLimiter};
        let mut limiter = RateLimiter::new(RateLimit {
            burst: 1,
            per_second: 1,
        });
        let r = router();
        let limits = Limits::default();
        let now = Duration::from_secs(1);
        for _ in 0..5 {
            let v: Value =
                serde_json::from_str(&r.handle_limited(b"{nope", &limits, &mut limiter, now))
                    .unwrap();
            assert_eq!(v["error"]["code"], "IPC_MALFORMED");
        }
        let ok: Value = serde_json::from_str(&r.handle_limited(
            br#"{"v":1,"id":1,"cmd":"echo"}"#,
            &limits,
            &mut limiter,
            now,
        ))
        .unwrap();
        assert_eq!(ok["ok"], true);
    }

    #[test]
    fn validated_commands_reject_bad_args_before_the_handler() {
        use std::cell::Cell;
        use std::rc::Rc;

        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let mut r = Router::new();
        r.register_validated(
            "set_volume",
            Schema::Struct(&[schema::Field {
                name: "level",
                schema: Schema::Int { min: 0, max: 10 },
            }]),
            move |_| {
                seen.set(seen.get() + 1);
                Ok(json!(null))
            },
        );

        let bad = reply(
            &r,
            r#"{"v":1,"id":3,"cmd":"set_volume","args":{"level":11}}"#,
        );
        assert_eq!(bad["error"]["code"], "IPC_INVALID_ARGS");
        assert_eq!(calls.get(), 0);

        let good = reply(
            &r,
            r#"{"v":1,"id":4,"cmd":"set_volume","args":{"level":5}}"#,
        );
        assert_eq!(good["ok"], true);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn error_details_are_sent_when_the_handler_gives_them() {
        let mut r = Router::new();
        r.register("quota", |_| {
            Err(CommandError::new("APP_QUOTA", "over quota").with_details(json!({ "used": 9 })))
        });
        let v = reply(&r, r#"{"v":1,"id":6,"cmd":"quota"}"#);
        assert_eq!(v["error"]["details"], json!({ "used": 9 }));
        assert_eq!(
            reply(&router(), r#"{"v":1,"id":7,"cmd":"fail"}"#)["error"].get("details"),
            None
        );
    }

    #[test]
    fn invalid_args_name_the_field_in_details() {
        let mut r = Router::new();
        r.register_validated(
            "set_volume",
            Schema::Struct(&[schema::Field {
                name: "level",
                schema: Schema::Int { min: 0, max: 10 },
            }]),
            |_| Ok(json!(null)),
        );
        let v = reply(
            &r,
            r#"{"v":1,"id":8,"cmd":"set_volume","args":{"level":11}}"#,
        );
        assert_eq!(v["error"]["details"], json!({ "path": "args.level" }));
    }

    #[test]
    fn later_registration_replaces_earlier() {
        let mut r = Router::new();
        r.register("x", |_| Ok(json!(1)));
        r.register("x", |_| Ok(json!(2)));
        assert_eq!(reply(&r, r#"{"v":1,"id":1,"cmd":"x"}"#)["data"], 2);
    }
}
