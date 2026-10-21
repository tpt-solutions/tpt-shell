// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! End-to-end checks for `#[command]`: the generated contract, the typed handler, and routing
//! through a real `Router`.

use serde_json::{json, Value};
use tpt_shell_ipc::router::{CommandError, Router};
use tpt_shell_ipc::Limits;
use tpt_shell_macros::{command, commands, register_commands};

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

fn reply(router: &Router, text: &str) -> Value {
    serde_json::from_str(&router.handle(text.as_bytes(), &Limits::default())).unwrap()
}

#[test]
fn the_contract_describes_each_command() {
    let contract = commands![list_dir, add_numbers];
    let text = contract.to_json_string();
    let doc: Value = serde_json::from_str(&text).unwrap();
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
fn registered_handlers_run_with_typed_arguments() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir, add_numbers);
    let ok = reply(
        &router,
        r#"{"v":1,"id":1,"cmd":"list_dir","args":{"path":"C:/d","depth":1}}"#,
    );
    assert_eq!(ok["data"], json!(["C:/d/a", "C:/d/b"]));
    let sum = reply(
        &router,
        r#"{"v":1,"id":2,"cmd":"add","args":{"x":2,"y":3}}"#,
    );
    assert_eq!(sum["data"], json!(5));
    let optional = reply(&router, r#"{"v":1,"id":3,"cmd":"add","args":{"x":2}}"#);
    assert_eq!(optional["data"], json!(2));
}

#[test]
fn schema_refusals_happen_before_the_function_runs() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let bad_type = reply(
        &router,
        r#"{"v":1,"id":4,"cmd":"list_dir","args":{"path":5,"depth":1}}"#,
    );
    assert_eq!(bad_type["error"]["code"], "IPC_INVALID_ARGS");
    let unknown = reply(
        &router,
        r#"{"v":1,"id":5,"cmd":"list_dir","args":{"path":"x","depth":1,"extra":true}}"#,
    );
    assert_eq!(unknown["error"]["code"], "IPC_INVALID_ARGS");
}

#[test]
fn errors_from_the_function_keep_their_code() {
    let mut router = Router::new();
    register_commands!(&mut router, list_dir);
    let err = reply(
        &router,
        r#"{"v":1,"id":6,"cmd":"list_dir","args":{"path":"x","depth":9}}"#,
    );
    assert_eq!(err["error"]["code"], "APP_TOO_DEEP");
}
