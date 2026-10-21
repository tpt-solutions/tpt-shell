// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Routes parsed requests to registered handlers and builds the response text (spec §5.5, §8).
//! Backend-independent: a platform backend passes each page message through [`Router::handle`]
//! and posts the returned text back to the page.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::json;

use tpt_shell_capabilities::{ConsentPrompt, Decision, Engine};

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
    guards: HashMap<String, Guard>,
}

/// What a guarded command acts on, read from its arguments and checked against the caller's
/// grant. [`Target::None`] matches only an unrestricted grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    None,
    Path(String),
    Device(String),
    Url(String),
    Bytes(u64),
}

impl Target {
    fn as_request(&self) -> tpt_shell_capabilities::Request<'_> {
        use tpt_shell_capabilities::Request;
        match self {
            Target::None => Request::None,
            Target::Path(path) => Request::Path(path),
            Target::Device(device) => Request::Device(device),
            Target::Url(url) => Request::Url(url),
            Target::Bytes(n) => Request::Bytes(*n),
        }
    }
}

/// Who sent a message. Grants are per window and per origin, so both are needed to check one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub window: String,
    pub origin: String,
}

type TargetOf = Box<dyn Fn(&Value) -> Target>;

struct Guard {
    capability: String,
    target_of: TargetOf,
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
        self.guards.remove(cmd);
        self.handlers.insert(cmd.to_string(), Box::new(handler));
    }

    /// Registers `handler` for `cmd`, behind `capability`. `target_of` reads what the call acts
    /// on from its arguments. Only [`Router::handle_authorized`] runs a guarded command; the
    /// unauthorized entry points refuse it.
    pub fn register_guarded(
        &mut self,
        cmd: &str,
        capability: &str,
        target_of: impl Fn(&Value) -> Target + 'static,
        handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
    ) {
        self.handlers.insert(cmd.to_string(), Box::new(handler));
        self.guards.insert(
            cmd.to_string(),
            Guard {
                capability: capability.to_string(),
                target_of: Box::new(target_of),
            },
        );
    }

    /// Registers `handler` for `cmd`, guarded by `schema`. The arguments are checked first; a
    /// bad value is answered with `IPC_INVALID_ARGS` and the handler never runs.
    pub fn register_validated(
        &mut self,
        cmd: &str,
        schema: Schema,
        handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
    ) {
        self.register(cmd, checked(schema, handler));
    }

    /// Registers `handler` for `cmd`, behind `capability`, with its arguments checked against
    /// `schema`. The grant is checked first, then the arguments, then the handler runs.
    pub fn register_guarded_validated(
        &mut self,
        cmd: &str,
        capability: &str,
        schema: Schema,
        target_of: impl Fn(&Value) -> Target + 'static,
        handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
    ) {
        self.register_guarded(cmd, capability, target_of, checked(schema, handler));
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

    /// The capability a command is registered behind, or `None` for an unguarded or unknown
    /// command.
    pub fn capability_of(&self, cmd: &str) -> Option<&str> {
        self.guards.get(cmd).map(|guard| guard.capability.as_str())
    }

    /// Like [`Router::handle`], but the message comes from `caller` and every command is
    /// capability-checked: the grant for `caller` must allow the call before its handler runs.
    /// A `consent` grant asks `prompt` the first time and then remembers the answer.
    pub fn handle_authorized(
        &self,
        caller: &Caller,
        engine: &mut Engine,
        prompt: &mut dyn ConsentPrompt,
        bytes: &[u8],
        limits: &Limits,
    ) -> String {
        let value = match parse_request(bytes, limits) {
            Ok(request) => self.dispatch_authorized(&request, caller, engine, prompt),
            Err(err) => parse_error_response(&err),
        };
        encode(&value)
    }

    fn dispatch(&self, request: &Request) -> Value {
        if self.guards.contains_key(&request.cmd) {
            return err_response(
                request.id,
                "CAP_NO_GRANT",
                &format!(
                    "`{}` needs a window and origin; it cannot run without them",
                    request.cmd
                ),
            );
        }
        match self.handlers.get(&request.cmd) {
            None => err_response(
                request.id,
                "IPC_UNKNOWN_COMMAND",
                &format!("no command named `{}`", request.cmd),
            ),
            Some(handler) => match handler(&request.args) {
                Ok(data) => ok_response(request.id, data),
                Err(err) => error_response(request.id, &err),
            },
        }
    }

    fn dispatch_authorized(
        &self,
        request: &Request,
        caller: &Caller,
        engine: &mut Engine,
        prompt: &mut dyn ConsentPrompt,
    ) -> Value {
        let Some(handler) = self.handlers.get(&request.cmd) else {
            return err_response(
                request.id,
                "IPC_UNKNOWN_COMMAND",
                &format!("no command named `{}`", request.cmd),
            );
        };
        let Some(guard) = self.guards.get(&request.cmd) else {
            return err_response(
                request.id,
                "CAP_NOT_DECLARED",
                &format!(
                    "`{}` declares no capability, so a page cannot call it",
                    request.cmd
                ),
            );
        };
        let target = (guard.target_of)(&request.args);
        let decision = engine.authorize(
            prompt,
            &caller.window,
            &caller.origin,
            &guard.capability,
            target.as_request(),
        );
        match decision {
            Decision::Allow => match handler(&request.args) {
                Ok(data) => ok_response(request.id, data),
                Err(err) => error_response(request.id, &err),
            },
            Decision::Deny(denial) => err_response(request.id, denial.code, &denial.message),
            Decision::NeedsConsent => err_response(
                request.id,
                "CAP_CONSENT_DECLINED",
                "the user has not answered the consent prompt",
            ),
        }
    }
}

/// The error response for a failed command, with its details when it has some.
fn error_response(id: u64, err: &CommandError) -> Value {
    let mut response = err_response(id, &err.code, &err.message);
    if let Some(details) = &err.details {
        response["error"]["details"] = details.clone();
    }
    response
}

/// Wraps `handler` so that its arguments are checked against `schema` first. A bad value is
/// answered with `IPC_INVALID_ARGS` and the handler never runs.
fn checked(
    schema: Schema,
    handler: impl Fn(&Value) -> Result<Value, CommandError> + 'static,
) -> impl Fn(&Value) -> Result<Value, CommandError> + 'static {
    move |args| {
        validate(&schema, args, "args").map_err(|e| {
            CommandError::new(schema::INVALID_ARGS, &e.to_string())
                .with_details(json!({ "path": e.path }))
        })?;
        handler(args)
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

    #[test]
    fn capability_of_names_the_grant_a_guarded_command_needs() {
        let mut router = Router::new();
        router.register_guarded("read", "fs.read", |_| Target::None, |_| Ok(json!(null)));
        router.register("ping", |_| Ok(json!("pong")));
        assert_eq!(router.capability_of("read"), Some("fs.read"));
        assert_eq!(router.capability_of("ping"), None);
        assert_eq!(router.capability_of("missing"), None);
    }

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

#[cfg(test)]
mod authorized_tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;
    use std::rc::Rc;
    use tpt_shell_capabilities::{Grant, Scope};

    /// Answers consent prompts with `answer` and counts how many times it was asked.
    struct Ask {
        answer: bool,
        asked: usize,
    }

    impl ConsentPrompt for Ask {
        fn ask(&mut self, _window: &str, _capability: &str) -> bool {
            self.asked += 1;
            self.answer
        }
    }

    fn caller(window: &str) -> Caller {
        Caller {
            window: window.into(),
            origin: "https://tpt-app.localhost".into(),
        }
    }

    fn grant(window: &str, scope: Scope, consent: bool) -> Grant {
        Grant {
            capability: "fs.read".into(),
            window: window.into(),
            origin: "https://tpt-app.localhost".into(),
            scope,
            consent,
        }
    }

    /// A router with one guarded command, `read`, that counts its calls.
    fn router(calls: Rc<Cell<u32>>) -> Router {
        let mut r = Router::new();
        r.register_guarded(
            "read",
            "fs.read",
            |args| match args["path"].as_str() {
                Some(path) => Target::Path(path.to_string()),
                None => Target::None,
            },
            move |_| {
                calls.set(calls.get() + 1);
                Ok(json!("contents"))
            },
        );
        r.register("unguarded", |_| Ok(json!(null)));
        r
    }

    fn send(r: &Router, engine: &mut Engine, prompt: &mut Ask, window: &str, msg: &str) -> Value {
        let text = r.handle_authorized(
            &caller(window),
            engine,
            prompt,
            msg.as_bytes(),
            &Limits::default(),
        );
        serde_json::from_str(&text).unwrap()
    }

    const READ_IN_SCOPE: &str =
        r#"{"v":1,"id":1,"cmd":"read","args":{"path":"C:/data/notes.txt"}}"#;

    #[test]
    fn an_in_scope_call_runs_the_handler() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Paths(vec!["C:/data".into()]), false))
            .unwrap();
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            READ_IN_SCOPE,
        );
        assert_eq!(v["ok"], true);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn an_out_of_scope_call_is_refused_before_the_handler() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Paths(vec!["C:/data".into()]), false))
            .unwrap();
        let msg = r#"{"v":1,"id":2,"cmd":"read","args":{"path":"C:/secrets/key"}}"#;
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            msg,
        );
        assert_eq!(v["error"]["code"], "CAP_OUT_OF_SCOPE");
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_grant_belongs_to_one_window() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Unrestricted, false))
            .unwrap();
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "other",
            READ_IN_SCOPE,
        );
        assert_eq!(v["error"]["code"], "CAP_NO_GRANT");
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_call_without_a_target_needs_an_unrestricted_grant() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Paths(vec!["C:/data".into()]), false))
            .unwrap();
        let msg = r#"{"v":1,"id":3,"cmd":"read","args":{}}"#;
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            msg,
        );
        assert_eq!(v["error"]["code"], "CAP_BAD_REQUEST");
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_guarded_command_cannot_run_through_the_unauthorized_entry_point() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let v: Value =
            serde_json::from_str(&r.handle(READ_IN_SCOPE.as_bytes(), &Limits::default())).unwrap();
        assert_eq!(v["error"]["code"], "CAP_NO_GRANT");
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn a_command_without_a_capability_is_refused_on_the_authorized_path() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls);
        let mut engine = Engine::new();
        let msg = r#"{"v":1,"id":4,"cmd":"unguarded"}"#;
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            msg,
        );
        assert_eq!(v["error"]["code"], "CAP_NOT_DECLARED");
    }

    #[test]
    fn a_consent_grant_asks_once_and_then_remembers_the_answer() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Unrestricted, true))
            .unwrap();
        let mut prompt = Ask {
            answer: true,
            asked: 0,
        };
        assert_eq!(
            send(&r, &mut engine, &mut prompt, "main", READ_IN_SCOPE)["ok"],
            true
        );
        assert_eq!(
            send(&r, &mut engine, &mut prompt, "main", READ_IN_SCOPE)["ok"],
            true
        );
        assert_eq!(prompt.asked, 1);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_declined_consent_refuses_the_call() {
        let calls = Rc::new(Cell::new(0));
        let r = router(calls.clone());
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Unrestricted, true))
            .unwrap();
        let mut prompt = Ask {
            answer: false,
            asked: 0,
        };
        let v = send(&r, &mut engine, &mut prompt, "main", READ_IN_SCOPE);
        assert_eq!(v["error"]["code"], "CAP_CONSENT_DECLINED");
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn handler_errors_keep_their_details_on_the_authorized_path() {
        let mut r = Router::new();
        r.register_guarded(
            "quota",
            "fs.read",
            |_| Target::None,
            |_| Err(CommandError::new("APP_QUOTA", "over").with_details(json!({ "used": 9 }))),
        );
        let mut engine = Engine::new();
        engine
            .add_grant(grant("main", Scope::Unrestricted, false))
            .unwrap();
        let msg = r#"{"v":1,"id":5,"cmd":"quota"}"#;
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            msg,
        );
        assert_eq!(v["error"]["details"], json!({ "used": 9 }));
    }

    #[test]
    fn registering_a_command_again_without_a_capability_removes_its_guard() {
        let calls = Rc::new(Cell::new(0));
        let mut r = router(calls.clone());
        r.register("read", |_| Ok(json!("open")));
        let mut engine = Engine::new();
        let v = send(
            &r,
            &mut engine,
            &mut Ask {
                answer: true,
                asked: 0,
            },
            "main",
            READ_IN_SCOPE,
        );
        assert_eq!(v["error"]["code"], "CAP_NOT_DECLARED");
    }
}
