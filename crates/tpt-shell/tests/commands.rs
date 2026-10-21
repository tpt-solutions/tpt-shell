// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `#[command]` end to end, through the facade: the contract, the typed handler, and the
//! capability check on the authorized path.

use serde_json::{json, Value};
use tpt_shell::{
    command, commands, register_commands, Caller, CommandError, ConsentPrompt, Engine, Grant,
    Limits, Router, Scope,
};

#[command(capability = "fs.read")]
fn list_dir(path: String, depth: u8) -> Result<Vec<String>, CommandError> {
    if depth > 2 {
        return Err(CommandError::new("APP_TOO_DEEP", "depth above 2"));
    }
    Ok(vec![format!("{path}/a"), format!("{path}/b")])
}

#[command(name = "add", kind = "sync")]
fn add_numbers(x: i64, y: Option<i64>) -> i64 {
    x + y.unwrap_or(0)
}

const ORIGIN: &str = "https://tpt-app.localhost";

/// Refuses every consent prompt. None of these grants asks for consent.
struct Decline;

impl ConsentPrompt for Decline {
    fn ask(&mut self, _window: &str, _capability: &str) -> bool {
        false
    }
}

fn caller() -> Caller {
    Caller {
        window: "main".into(),
        origin: ORIGIN.into(),
    }
}

fn grant(scope: Scope) -> Grant {
    Grant {
        capability: "fs.read".into(),
        window: "main".into(),
        origin: ORIGIN.into(),
        scope,
        consent: false,
    }
}

fn ask(router: &Router, engine: &mut Engine, text: &str) -> Value {
    let reply = router.handle_authorized(
        &caller(),
        engine,
        &mut Decline,
        text.as_bytes(),
        &Limits::default(),
    );
    serde_json::from_str(&reply).unwrap()
}

fn unauthorized(router: &Router, text: &str) -> Value {
    serde_json::from_str(&router.handle(text.as_bytes(), &Limits::default())).unwrap()
}

const LIST: &str = r#"{"v":1,"id":1,"cmd":"list_dir","args":{"path":"C:/d","depth":1}}"#;

#[test]
fn the_contract_describes_each_command() {
    let contract = commands![list_dir, add_numbers];
    let doc: Value = serde_json::from_str(&contract.to_json_string()).unwrap();
    let commands = doc["commands"].as_array().unwrap();
    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["add", "list_dir"],
        "sorted by name, `name=` respected"
    );
    let list_dir = commands.iter().find(|c| c["name"] == "list_dir").unwrap();
    assert_eq!(list_dir["capability"], "fs.read");
    assert_eq!(list_dir["args"]["required"], json!(["path", "depth"]));
    assert_eq!(list_dir["args"]["properties"]["depth"]["maximum"], 255);
    let add = commands.iter().find(|c| c["name"] == "add").unwrap();
    assert_eq!(
        add["args"]["required"],
        json!(["x"]),
        "Option arguments are not required"
    );
}

#[test]
fn a_command_with_a_capability_runs_only_under_a_grant() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let mut engine = Engine::new();

    let refused = ask(&router, &mut engine, LIST);
    assert_eq!(refused["error"]["code"], "CAP_NO_GRANT");

    engine.add_grant(grant(Scope::Unrestricted)).unwrap();
    let ok = ask(&router, &mut engine, LIST);
    assert_eq!(ok["data"], json!(["C:/d/a", "C:/d/b"]));
}

#[test]
fn a_command_with_a_capability_cannot_run_through_the_unauthorized_path() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let refused = unauthorized(&router, LIST);
    assert_eq!(refused["error"]["code"], "CAP_NO_GRANT");
}

#[test]
fn a_scoped_grant_refuses_a_macro_command_because_it_has_no_target() {
    // The macro registers with no target, so only an unrestricted grant can allow the call.
    // A command that acts on a path is registered by hand with a target function.
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let mut engine = Engine::new();
    engine
        .add_grant(grant(Scope::Paths(vec!["C:/d".into()])))
        .unwrap();
    let reply = ask(&router, &mut engine, LIST);
    assert_eq!(reply["error"]["code"], "CAP_BAD_REQUEST");
}

#[test]
fn a_command_without_a_capability_is_refused_on_the_authorized_path() {
    let mut router = Router::new();
    register_commands!(&mut router, add_numbers);
    let mut engine = Engine::new();
    let refused = ask(
        &router,
        &mut engine,
        r#"{"v":1,"id":2,"cmd":"add","args":{"x":2}}"#,
    );
    assert_eq!(refused["error"]["code"], "CAP_NOT_DECLARED");
    let direct = unauthorized(
        &router,
        r#"{"v":1,"id":3,"cmd":"add","args":{"x":2,"y":3}}"#,
    );
    assert_eq!(direct["data"], json!(5));
}

#[test]
fn schema_refusals_happen_before_the_function_runs() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let mut engine = Engine::new();
    engine.add_grant(grant(Scope::Unrestricted)).unwrap();
    let bad_type = ask(
        &router,
        &mut engine,
        r#"{"v":1,"id":4,"cmd":"list_dir","args":{"path":5,"depth":1}}"#,
    );
    assert_eq!(bad_type["error"]["code"], "IPC_INVALID_ARGS");
    let unknown = ask(
        &router,
        &mut engine,
        r#"{"v":1,"id":5,"cmd":"list_dir","args":{"path":"x","depth":1,"extra":true}}"#,
    );
    assert_eq!(unknown["error"]["code"], "IPC_INVALID_ARGS");
}

#[test]
fn errors_from_the_function_keep_their_code() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let mut engine = Engine::new();
    engine.add_grant(grant(Scope::Unrestricted)).unwrap();
    let err = ask(
        &router,
        &mut engine,
        r#"{"v":1,"id":6,"cmd":"list_dir","args":{"path":"x","depth":9}}"#,
    );
    assert_eq!(err["error"]["code"], "APP_TOO_DEEP");
}

#[test]
fn optional_arguments_may_be_left_out() {
    let mut router = Router::new();
    register_commands!(&mut router, add_numbers);
    let two = unauthorized(&router, r#"{"v":1,"id":7,"cmd":"add","args":{"x":2}}"#);
    assert_eq!(two["data"], json!(2));
}
