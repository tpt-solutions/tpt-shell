// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! End-to-end tests of `#[command]`, `commands![]`, `register_commands!` and
//! `#[derive(Contract)]`: the generated contract and the router's handling of each kind.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};
use tpt_shell::__private::ipc::contract::{Flags, Kind};
use tpt_shell::__private::ipc::jobs::{JobContext, JobId};
use tpt_shell::__private::ipc::ScopedPath;
use tpt_shell::{command, commands, register_commands, Caller, CommandError, Contract, Limits};
use tpt_shell::{Engine, Router};
use tpt_shell_capabilities::{ConsentPrompt, Grant, Scope};

/// Answers no to every prompt. The tests grant what they need outright.
struct NoPrompt;

impl ConsentPrompt for NoPrompt {
    fn ask(&mut self, _: &str, _: &str) -> bool {
        false
    }
}

#[derive(Contract, Deserialize, Debug, PartialEq)]
struct Entry {
    #[contract(max_len = 8, chars = "abc")]
    name: String,
    #[contract(min = 0, max = 9)]
    level: u8,
    tags: Vec<String>,
}

#[derive(Contract, Deserialize, Debug, PartialEq)]
enum Mode {
    Read,
    Write,
}

#[command(capability = "fs.read", target(path = "path"))]
fn read_name(path: ScopedPath) -> Result<String, CommandError> {
    Ok(path.normalized())
}

#[command(name = "store")]
fn store_entry(entry: Entry, mode: Mode) -> String {
    format!("{}:{}:{:?}", entry.name, entry.level, mode)
}

#[command(kind = "async", ui)]
async fn later(n: u32) -> Result<u32, CommandError> {
    Ok(n + 1)
}

#[command(job, capability = "fs.read", target(path = "root"))]
fn scan(root: ScopedPath, job: &JobContext) -> Result<u32, CommandError> {
    job.progress(1, 2);
    Ok(root.as_str().len() as u32)
}

#[command(kind = "job", agent)]
fn count_first(job: &JobContext, n: u32) -> Result<u32, CommandError> {
    job.progress(0, 1);
    Ok(n * 2)
}

#[command(kind = "stream", capability = "fs.read")]
fn numbers(n: u32) -> impl Iterator<Item = u32> {
    0..n
}

#[command(kind = "stream")]
fn checked_numbers(n: u32) -> Result<impl Iterator<Item = u32>, CommandError> {
    if n > 9 {
        return Err(CommandError::new("TOO_MANY", "at most nine items"));
    }
    Ok(0..n)
}

#[command(agent = false)]
fn ping() -> String {
    "pong".into()
}

fn full_router() -> Router {
    let mut router = Router::new();
    register_commands!(
        &mut router,
        read_name,
        store_entry,
        later,
        scan,
        count_first,
        numbers,
        checked_numbers,
        ping
    );
    router
}

/// Takes every item a streamed job sends, returning credit as each item is taken, until the
/// stream is closed and empty.
fn drain_stream(router: &mut Router, job: u64) -> Vec<String> {
    let jobs = router.jobs();
    let id = JobId(job);
    let mut items = Vec::new();
    let begin = Instant::now();
    loop {
        let taken = jobs.borrow_mut().stream_take(id);
        match taken {
            Some(chunk) => {
                items.push(String::from_utf8(chunk).expect("a JSON item"));
                jobs.borrow_mut().stream_grant(id, 1);
            }
            None => {
                if !jobs.borrow().stream_is_open(id) {
                    return items;
                }
                assert!(
                    begin.elapsed() < Duration::from_secs(10),
                    "stream did not close"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn engine_granting_fs_read() -> Engine {
    let mut engine = Engine::new();
    engine
        .add_grant(Grant {
            capability: "fs.read".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Unrestricted,
            consent: false,
        })
        .unwrap();
    engine
}

fn caller() -> Caller {
    Caller {
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
    }
}

fn request(router: &Router, cmd: &str, args: Value) -> Value {
    let text = json!({ "v": 1, "id": 1, "cmd": cmd, "args": args }).to_string();
    let reply = router.handle(text.as_bytes(), &Limits::default());
    serde_json::from_str(&reply).unwrap()
}

fn authorized(router: &Router, engine: &mut Engine, cmd: &str, args: Value) -> Value {
    let text = json!({ "v": 1, "id": 2, "cmd": cmd, "args": args }).to_string();
    let reply = router.handle_authorized(
        &caller(),
        engine,
        &mut NoPrompt,
        text.as_bytes(),
        &Limits::default(),
    );
    serde_json::from_str(&reply).unwrap()
}

#[test]
fn the_contract_records_each_kind_and_flag() {
    let contract = commands![read_name, store_entry, later, scan, count_first, ping];
    let kind = |name: &str| contract.commands().find(|c| c.name == name).map(|c| c.kind);
    assert_eq!(kind("read_name"), Some(Kind::Sync));
    assert_eq!(kind("store"), Some(Kind::Sync));
    assert_eq!(kind("later"), Some(Kind::Async));
    assert_eq!(kind("scan"), Some(Kind::Job));
    assert_eq!(kind("count_first"), Some(Kind::Job));
    assert_eq!(
        contract.flags("later"),
        Some(Flags {
            ui: true,
            agent: false
        })
    );
    assert_eq!(
        contract.flags("count_first"),
        Some(Flags {
            ui: false,
            agent: true
        })
    );
    assert_eq!(
        contract.flags("ping"),
        Some(Flags {
            ui: false,
            agent: false
        })
    );
    assert_eq!(contract.flags("missing"), None);
}

#[test]
fn derived_structs_and_enums_become_schemas() {
    let contract = commands![store_entry];
    let json = contract.to_json_string();
    assert!(json.contains("\"maxLength\": 8"), "{json}");
    assert!(json.contains("\"pattern\": \"^[abc]*$\""), "{json}");
    assert!(json.contains("\"maximum\": 9"), "{json}");
    assert!(json.contains("\"enum\": ["), "{json}");
    assert!(json.contains("\"Write\""), "{json}");
}

#[test]
fn a_derived_struct_argument_is_validated_before_the_handler() {
    let router = full_router();
    let ok = request(
        &router,
        "store",
        json!({ "entry": { "name": "abc", "level": 3, "tags": [] }, "mode": "Read" }),
    );
    assert_eq!(ok["data"], "abc:3:Read");

    let too_long = request(
        &router,
        "store",
        json!({ "entry": { "name": "abcabcabcabc", "level": 3, "tags": [] }, "mode": "Read" }),
    );
    assert_eq!(too_long["error"]["code"], "IPC_INVALID_ARGS");

    let bad_level = request(
        &router,
        "store",
        json!({ "entry": { "name": "abc", "level": 12, "tags": [] }, "mode": "Read" }),
    );
    assert_eq!(bad_level["error"]["code"], "IPC_INVALID_ARGS");

    let bad_mode = request(
        &router,
        "store",
        json!({ "entry": { "name": "abc", "level": 3, "tags": [] }, "mode": "Delete" }),
    );
    assert_eq!(bad_mode["error"]["code"], "IPC_INVALID_ARGS");
}

#[test]
fn an_async_command_is_run_to_completion() {
    let router = full_router();
    let reply = request(&router, "later", json!({ "n": 4 }));
    assert_eq!(reply["data"], 5);
}

#[test]
fn a_scoped_path_argument_is_refused_when_unsafe() {
    let mut router = Router::new();
    register_commands!(&mut router, read_name);
    let mut engine = engine_granting_fs_read();
    let bad = authorized(
        &router,
        &mut engine,
        "read_name",
        json!({ "path": "../etc" }),
    );
    assert_eq!(bad["error"]["code"], "IPC_INVALID_ARGS");
    let good = authorized(
        &router,
        &mut engine,
        "read_name",
        json!({ "path": "C:/Data/a" }),
    );
    assert_eq!(good["data"], "c:/data/a");
}

#[test]
fn a_job_command_answers_with_an_id_and_reports_its_result() {
    let router = full_router();
    let mut engine = engine_granting_fs_read();
    let started = authorized(&router, &mut engine, "scan", json!({ "root": "C:/Data" }));
    let job = started["data"]["job"].as_u64().expect("a job id");

    let mut seen: Vec<Value> = Vec::new();
    let begin = Instant::now();
    while begin.elapsed() < Duration::from_secs(10)
        && !seen.iter().any(|e| e["event"] == "job.result")
    {
        seen.extend(router.drain_job_events());
        std::thread::sleep(Duration::from_millis(2));
    }
    let result = seen
        .iter()
        .find(|e| e["event"] == "job.result")
        .expect("a result event");
    assert_eq!(result["data"]["job"], job);
    assert_eq!(result["data"]["data"], 7);
}

#[test]
fn a_job_command_with_the_context_last_or_first_works_the_same() {
    let router = full_router();
    let started = request(&router, "count_first", json!({ "n": 21 }));
    assert!(started["data"]["job"].as_u64().is_some(), "{started}");
}

#[test]
fn a_job_command_needs_its_capability_like_any_other() {
    let router = full_router();
    let mut engine = Engine::new();
    let refused = authorized(&router, &mut engine, "scan", json!({ "root": "C:/Data" }));
    assert_eq!(refused["error"]["code"], "CAP_NO_GRANT");
}

#[test]
fn a_sync_command_still_runs_without_flags() {
    let router = full_router();
    assert_eq!(request(&router, "ping", json!({}))["data"], "pong");
}

#[test]
fn a_stream_command_sends_each_item_in_order_under_credit() {
    let mut router = full_router();
    let mut engine = engine_granting_fs_read();
    let started = authorized(&router, &mut engine, "numbers", json!({ "n": 20 }));
    let job = started["data"]["job"].as_u64().expect("a job id");
    // Eight items fit in the initial credit, so the rest wait for the grants drain_stream returns.
    let items = drain_stream(&mut router, job);
    assert_eq!(items, (0..20).map(|n| n.to_string()).collect::<Vec<_>>());
}

#[test]
fn a_stream_command_that_refuses_its_arguments_fails_the_job_with_its_code() {
    let router = full_router();
    let started = request(&router, "checked_numbers", json!({ "n": 12 }));
    let job = started["data"]["job"].as_u64().expect("a job id");
    let mut seen: Vec<Value> = Vec::new();
    let begin = Instant::now();
    while begin.elapsed() < Duration::from_secs(10)
        && !seen.iter().any(|e| e["event"] == "job.failed")
    {
        seen.extend(router.drain_job_events());
        std::thread::sleep(Duration::from_millis(2));
    }
    let failed = seen
        .iter()
        .find(|e| e["event"] == "job.failed")
        .expect("a failed event");
    assert_eq!(failed["data"]["job"], job);
    assert_eq!(failed["data"]["code"], "TOO_MANY");
}

#[test]
fn a_stream_command_needs_its_capability_like_any_other() {
    let router = full_router();
    let mut engine = Engine::new();
    let refused = authorized(&router, &mut engine, "numbers", json!({ "n": 3 }));
    assert!(refused["data"]["job"].is_null(), "{refused}");
    assert_eq!(refused["error"]["code"], "CAP_NO_GRANT");
}

#[test]
fn a_stream_command_is_recorded_as_a_job_until_the_page_transport_exists() {
    let contract = commands![numbers, checked_numbers];
    let kind = |name: &str| contract.commands().find(|c| c.name == name).map(|c| c.kind);
    assert_eq!(kind("numbers"), Some(Kind::Job));
    assert_eq!(kind("checked_numbers"), Some(Kind::Job));
}
