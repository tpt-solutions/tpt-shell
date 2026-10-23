// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Security boundaries for the page-to-shell path, end to end: a call must pass the envelope
//! checks, then the grant and scope checks, before a handler runs. Each test names the rule it
//! checks. Unit tests in the crate cover the parts; these tests cover the whole path.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use tpt_shell_capabilities::{ConsentPrompt, Engine, Grant, Scope};
use tpt_shell_ipc::rate_limit::{RateLimit, RateLimiter};
use tpt_shell_ipc::{Caller, Limits, Router, Target};

/// A prompt that must never be asked in these tests; it records any question and refuses.
#[derive(Default)]
struct RefusingPrompt {
    asked: u32,
}

impl ConsentPrompt for RefusingPrompt {
    fn ask(&mut self, _window: &str, _capability: &str) -> bool {
        self.asked += 1;
        false
    }
}

fn caller(window: &str, origin: &str) -> Caller {
    Caller {
        window: window.to_string(),
        origin: origin.to_string(),
    }
}

fn grant(capability: &str, window: &str, origin: &str, scope: Scope) -> Grant {
    Grant {
        capability: capability.to_string(),
        window: window.to_string(),
        origin: origin.to_string(),
        scope,
        consent: false,
    }
}

fn envelope(cmd: &str, args: Value) -> Vec<u8> {
    json!({ "v": 1, "id": 7, "cmd": cmd, "args": args })
        .to_string()
        .into_bytes()
}

/// A router with one guarded `fetch` command that reads `args.url` as its target. Each run of
/// the handler is counted, so a test can show a refused call never reached it.
fn url_router() -> (Router, Rc<Cell<u32>>) {
    let runs = Rc::new(Cell::new(0));
    let counter = runs.clone();
    let mut router = Router::new();
    router.register_guarded(
        "fetch",
        "net.fetch",
        |args| Target::Url(args["url"].as_str().unwrap_or("").to_string()),
        move |_args| {
            counter.set(counter.get() + 1);
            Ok(json!("fetched"))
        },
    );
    (router, runs)
}

/// A router with one guarded `read` command that reads `args.path` as its target.
fn path_router() -> (Router, Rc<Cell<u32>>) {
    let runs = Rc::new(Cell::new(0));
    let counter = runs.clone();
    let mut router = Router::new();
    router.register_guarded(
        "read",
        "fs.read",
        |args| Target::Path(args["path"].as_str().unwrap_or("").to_string()),
        move |_args| {
            counter.set(counter.get() + 1);
            Ok(json!("read"))
        },
    );
    (router, runs)
}

/// A router with one unguarded `ping` command, counted. The rate limiter sits on the
/// unauthorized entry point, which only runs commands that declare no capability.
fn ping_router() -> (Router, Rc<Cell<u32>>) {
    let runs = Rc::new(Cell::new(0));
    let counter = runs.clone();
    let mut router = Router::new();
    router.register("ping", move |_args| {
        counter.set(counter.get() + 1);
        Ok(json!("pong"))
    });
    (router, runs)
}

fn run(router: &Router, engine: &mut Engine, who: &Caller, bytes: &[u8], limits: &Limits) -> Value {
    let mut prompt = RefusingPrompt::default();
    let text = router.handle_authorized(who, engine, &mut prompt, bytes, limits);
    serde_json::from_str(&text).expect("the router always answers with JSON")
}

fn code_of(response: &Value) -> &str {
    response["error"]["code"].as_str().unwrap_or("")
}

// ---- capability bypass: the grant is checked before the handler runs ----

#[test]
fn guarded_command_with_no_grant_never_runs_its_handler() {
    let (router, runs) = url_router();
    let mut engine = Engine::new();
    let who = caller("main", "https://tpt-app.localhost");
    let response = run(
        &router,
        &mut engine,
        &who,
        &envelope("fetch", json!({ "url": "https://example.com/" })),
        &Limits::default(),
    );
    assert_eq!(code_of(&response), "CAP_NO_GRANT");
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_grant_for_another_window_does_not_allow_this_window() {
    let (router, runs) = url_router();
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "net.fetch",
            "settings",
            "https://tpt-app.localhost",
            Scope::Urls(vec!["https://example.com/".into()]),
        ))
        .unwrap();
    let response = run(
        &router,
        &mut engine,
        &caller("main", "https://tpt-app.localhost"),
        &envelope("fetch", json!({ "url": "https://example.com/" })),
        &Limits::default(),
    );
    assert_eq!(code_of(&response), "CAP_NO_GRANT");
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_grant_for_another_origin_does_not_allow_this_origin() {
    let (router, runs) = url_router();
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "net.fetch",
            "main",
            "https://tpt-app.localhost",
            Scope::Urls(vec!["https://example.com/".into()]),
        ))
        .unwrap();
    let response = run(
        &router,
        &mut engine,
        &caller("main", "https://evil.example"),
        &envelope("fetch", json!({ "url": "https://example.com/" })),
        &Limits::default(),
    );
    assert_eq!(code_of(&response), "CAP_NO_GRANT");
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_target_outside_the_granted_url_scope_is_refused_before_the_handler() {
    let (router, runs) = url_router();
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "net.fetch",
            "main",
            "https://tpt-app.localhost",
            Scope::Urls(vec!["https://example.com/app".into()]),
        ))
        .unwrap();
    let who = caller("main", "https://tpt-app.localhost");
    for url in [
        "https://example.com.evil.test/app",
        "https://example.com/appendix",
        "https://evil.test/https://example.com/app",
        "http://example.com/app",
    ] {
        let response = run(
            &router,
            &mut engine,
            &who,
            &envelope("fetch", json!({ "url": url })),
            &Limits::default(),
        );
        assert!(
            !response["ok"].as_bool().unwrap_or(true),
            "{url} should be refused"
        );
        assert_eq!(code_of(&response), "CAP_OUT_OF_SCOPE", "{url}");
    }
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_narrowed_grant_refuses_what_the_wider_grant_allowed() {
    // The engine has no revoke call: a grant is taken away by building the engine again with
    // the narrower grant, which is what this test checks.
    let (router, runs) = url_router();
    let who = caller("main", "https://tpt-app.localhost");
    let call = envelope("fetch", json!({ "url": "https://example.com/app/page" }));

    let mut wide = Engine::new();
    wide.add_grant(grant(
        "net.fetch",
        "main",
        "https://tpt-app.localhost",
        Scope::Urls(vec!["https://example.com/app".into()]),
    ))
    .unwrap();
    assert_eq!(
        run(&router, &mut wide, &who, &call, &Limits::default())["ok"],
        true
    );

    let mut narrow = Engine::new();
    narrow
        .add_grant(grant(
            "net.fetch",
            "main",
            "https://tpt-app.localhost",
            Scope::Urls(vec!["https://example.com/app/other".into()]),
        ))
        .unwrap();
    let refused = run(&router, &mut narrow, &who, &call, &Limits::default());
    assert_eq!(code_of(&refused), "CAP_OUT_OF_SCOPE");
    assert_eq!(runs.get(), 1, "only the call under the wide grant ran");
}

#[test]
fn a_call_with_no_target_is_refused_by_a_scoped_grant() {
    // A guarded command whose target is missing reads as Target::None, which only an
    // unrestricted grant allows; a scoped grant must refuse it.
    let mut router = Router::new();
    let runs = Rc::new(Cell::new(0));
    let counter = runs.clone();
    router.register_guarded(
        "fetch",
        "net.fetch",
        |_args| Target::None,
        move |_args| {
            counter.set(counter.get() + 1);
            Ok(json!(null))
        },
    );
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "net.fetch",
            "main",
            "https://tpt-app.localhost",
            Scope::Urls(vec!["https://example.com/".into()]),
        ))
        .unwrap();
    let response = run(
        &router,
        &mut engine,
        &caller("main", "https://tpt-app.localhost"),
        &envelope("fetch", json!({})),
        &Limits::default(),
    );
    assert_ne!(response["ok"], true);
    assert_eq!(runs.get(), 0);
}

// ---- malformed and oversized envelopes: stable codes, no handler, no panic ----

fn assert_refused_without_running(router: &Router, runs: &Rc<Cell<u32>>, bytes: &[u8], code: &str) {
    let limits = Limits::default();
    let mut engine = Engine::new();
    let mut prompt = RefusingPrompt::default();
    let text = router.handle_authorized(
        &caller("main", "https://tpt-app.localhost"),
        &mut engine,
        &mut prompt,
        bytes,
        &limits,
    );
    let response: Value = serde_json::from_str(&text).expect("JSON answer");
    assert_eq!(response["ok"], false, "{text}");
    assert_eq!(
        response["id"],
        Value::Null,
        "a refused envelope has no id: {text}"
    );
    assert_eq!(code_of(&response), code, "{text}");
    assert_eq!(runs.get(), 0, "no handler may run for a refused envelope");
}

#[test]
fn an_oversized_message_is_refused_with_its_code() {
    let (router, runs) = url_router();
    let limits = Limits {
        max_message_bytes: 64,
        ..Limits::default()
    };
    let mut engine = Engine::new();
    let big = format!(
        "{{\"v\":1,\"id\":1,\"cmd\":\"fetch\",\"args\":{{\"url\":\"{}\"}}}}",
        "a".repeat(200)
    );
    let response = run(
        &router,
        &mut engine,
        &caller("main", "o"),
        big.as_bytes(),
        &limits,
    );
    assert_eq!(code_of(&response), "IPC_TOO_LARGE");
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_message_over_the_default_size_cap_is_refused_without_parsing_it() {
    let (router, runs) = url_router();
    let padding = " ".repeat(Limits::default().max_message_bytes + 1);
    let bytes = format!("{padding}{{\"v\":1,\"id\":1,\"cmd\":\"fetch\"}}");
    assert_refused_without_running(&router, &runs, bytes.as_bytes(), "IPC_TOO_LARGE");
}

#[test]
fn a_nested_argument_deeper_than_the_cap_is_refused() {
    let (router, runs) = url_router();
    let limits = Limits::default();
    let mut nested = json!(0);
    for _ in 0..(limits.max_depth + 4) {
        nested = json!([nested]);
    }
    let bytes = envelope(
        "fetch",
        json!({ "url": "https://example.com/", "deep": nested }),
    );
    let mut engine = Engine::new();
    let response = run(&router, &mut engine, &caller("main", "o"), &bytes, &limits);
    assert_eq!(code_of(&response), "IPC_TOO_DEEP");
    assert_eq!(runs.get(), 0);
}

#[test]
fn nesting_far_past_the_parser_limit_is_refused_not_a_crash() {
    // The JSON parser stops at its own recursion limit before our depth check runs. Either code
    // is a refusal; what matters is that nothing panics and no handler runs.
    let (router, runs) = url_router();
    let depth = 2000;
    let text = format!(
        "{{\"v\":1,\"id\":1,\"cmd\":\"fetch\",\"args\":{}{}}}",
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let mut engine = Engine::new();
    let response = run(
        &router,
        &mut engine,
        &caller("main", "o"),
        text.as_bytes(),
        &Limits::default(),
    );
    let code = code_of(&response);
    assert!(
        code == "IPC_TOO_DEEP" || code == "IPC_MALFORMED",
        "unexpected code {code}"
    );
    assert_eq!(runs.get(), 0);
}

#[test]
fn a_truncated_message_is_malformed() {
    let (router, runs) = url_router();
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":7,\"cmd\":\"fetch\",\"args\":{\"url\":\"https://exa",
        "IPC_MALFORMED",
    );
}

#[test]
fn non_json_bytes_are_malformed() {
    let (router, runs) = url_router();
    assert_refused_without_running(&router, &runs, &[0xff, 0xfe, 0x00, 0x01], "IPC_MALFORMED");
    assert_refused_without_running(&router, &runs, b"", "IPC_MALFORMED");
    assert_refused_without_running(&router, &runs, b"not json at all", "IPC_MALFORMED");
}

#[test]
fn a_json_value_that_is_not_an_object_is_refused() {
    let (router, runs) = url_router();
    assert_refused_without_running(&router, &runs, b"[1,2,3]", "IPC_NOT_OBJECT");
    assert_refused_without_running(&router, &runs, b"\"fetch\"", "IPC_NOT_OBJECT");
}

#[test]
fn a_wrong_envelope_version_is_refused_with_the_version_code() {
    let (router, runs) = url_router();
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":2,\"id\":1,\"cmd\":\"fetch\"}",
        "IPC_UNSUPPORTED_VERSION",
    );
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":\"1\",\"id\":1,\"cmd\":\"fetch\"}",
        "IPC_WRONG_TYPE",
    );
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"id\":1,\"cmd\":\"fetch\"}",
        "IPC_MISSING_FIELD",
    );
}

#[test]
fn an_unexpected_envelope_field_is_refused() {
    // A field the envelope does not define must not be ignored: it could be an attempt to
    // smuggle a target past the checks.
    let (router, runs) = url_router();
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":1,\"cmd\":\"fetch\",\"scope\":\"*\"}",
        "IPC_UNKNOWN_FIELD",
    );
}

#[test]
fn bad_ids_and_arguments_are_refused_by_type() {
    let (router, runs) = url_router();
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":-1,\"cmd\":\"fetch\"}",
        "IPC_WRONG_TYPE",
    );
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":1.5,\"cmd\":\"fetch\"}",
        "IPC_WRONG_TYPE",
    );
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":1,\"cmd\":\"fetch\",\"args\":[\"https://example.com/\"]}",
        "IPC_WRONG_TYPE",
    );
}

#[test]
fn command_names_must_be_present_non_empty_and_short() {
    let (router, runs) = url_router();
    assert_refused_without_running(
        &router,
        &runs,
        b"{\"v\":1,\"id\":1,\"cmd\":\"\"}",
        "IPC_EMPTY_COMMAND",
    );
    let long = "c".repeat(Limits::default().max_cmd_len + 1);
    let bytes = format!("{{\"v\":1,\"id\":1,\"cmd\":\"{long}\"}}");
    assert_refused_without_running(&router, &runs, bytes.as_bytes(), "IPC_COMMAND_TOO_LONG");
}

#[test]
fn a_flood_of_calls_is_rate_limited_after_the_burst() {
    let (router, runs) = ping_router();
    let mut limiter = RateLimiter::new(RateLimit {
        burst: 3,
        per_second: 1,
    });
    let bytes = envelope("ping", json!({}));
    let mut limited = 0;
    for _ in 0..10 {
        let text = router.handle_limited(&bytes, &Limits::default(), &mut limiter, Duration::ZERO);
        let response: Value = serde_json::from_str(&text).unwrap();
        if code_of(&response) == "IPC_RATE_LIMITED" {
            limited += 1;
        }
    }
    assert_eq!(
        runs.get(),
        3,
        "only the burst reaches the handler at one instant"
    );
    assert_eq!(limited, 7);
}

#[test]
fn a_malformed_message_does_not_spend_a_rate_limit_token() {
    // The size cap bounds the cost of a bad message; it must not be able to drain the bucket
    // for the valid calls that follow.
    let (router, runs) = ping_router();
    let mut limiter = RateLimiter::new(RateLimit {
        burst: 1,
        per_second: 1,
    });
    let limits = Limits::default();
    for _ in 0..5 {
        router.handle_limited(b"{not json", &limits, &mut limiter, Duration::ZERO);
    }
    let text = router.handle_limited(
        &envelope("ping", json!({})),
        &limits,
        &mut limiter,
        Duration::ZERO,
    );
    let response: Value = serde_json::from_str(&text).unwrap();
    // The valid call still has its token; it is refused by the grant check only if the grant
    // is missing, which it is not here, so it runs.
    assert_eq!(response["ok"], true, "{text}");
    assert_eq!(runs.get(), 1);
}

// ---- path escapes: the scope rules hold for a command that reads a path ----

/// A root the grant covers and paths that try to leave it, for the current platform.
fn path_case() -> (&'static str, &'static [&'static str], &'static str) {
    if cfg!(windows) {
        (
            "C:/Data",
            &[
                "C:/Data/../Secrets/key.txt",
                "C:/Data/sub/../../Secrets/key.txt",
                "C:/Data\\..\\Secrets\\key.txt",
                "C:/Other/key.txt",
                "C:/Data2/key.txt",
                "C:secret.txt",
                "\\\\server\\share\\key.txt",
                "\\\\?\\C:\\Data\\key.txt",
                "C:/Data/key.txt:hidden",
                "C:/Data/CON",
                "C:/Data/trailing.",
                "C:/Data/key\0.txt",
            ],
            "C:/Data/report.txt",
        )
    } else {
        (
            "/data",
            &[
                "/data/../secrets/key.txt",
                "/data/sub/../../secrets/key.txt",
                "/other/key.txt",
                "/data2/key.txt",
                "relative/key.txt",
                "/data/key\0.txt",
            ],
            "/data/report.txt",
        )
    }
}

#[test]
fn paths_outside_the_granted_root_are_refused_before_the_handler() {
    let (root, escapes, _) = path_case();
    let (router, runs) = path_router();
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "fs.read",
            "main",
            "o",
            Scope::Paths(vec![root.to_string()]),
        ))
        .unwrap();
    let who = caller("main", "o");
    for path in escapes {
        let response = run(
            &router,
            &mut engine,
            &who,
            &envelope("read", json!({ "path": path })),
            &Limits::default(),
        );
        assert_ne!(
            response["ok"], true,
            "{path:?} escaped the root: {response}"
        );
        let code = code_of(&response);
        assert!(
            code == "CAP_OUT_OF_SCOPE" || code == "CAP_BAD_REQUEST",
            "{path:?} refused with {code}"
        );
    }
    assert_eq!(runs.get(), 0, "no escaping path reached the handler");
}

#[test]
fn a_path_inside_the_granted_root_still_runs() {
    let (root, _, inside) = path_case();
    let (router, runs) = path_router();
    let mut engine = Engine::new();
    engine
        .add_grant(grant(
            "fs.read",
            "main",
            "o",
            Scope::Paths(vec![root.to_string()]),
        ))
        .unwrap();
    let response = run(
        &router,
        &mut engine,
        &caller("main", "o"),
        &envelope("read", json!({ "path": inside })),
        &Limits::default(),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(runs.get(), 1);
}
