// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A scripted agent session against a notes app. It runs the agent policy from start to finish
//! and prints each reply, so the rules can be seen working:
//!
//!   1. a normal tool call runs and is logged,
//!   2. a call with bad arguments is refused before the handler runs,
//!   3. a sensitive tool asks the user, and the user says no (the example declines),
//!   4. the rate limit refuses the call past the burst,
//!   5. the kill switch ends every later call at once.
//!
//! The session logs every call (`session.log`), so each action is on record. This runs on any
//! OS and needs no window. It does not drive a web page: the agent's page actions are not
//! offered as tools yet (todo.md, M9).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{json, Value};
use tpt_shell_agent::{AgentTool, Outcome, Policy, Session};
use tpt_shell_ipc::rate_limit::RateLimit;
use tpt_shell_ipc::schema::{Field, Schema};

const TEXT_FIELDS: &[Field] = &[Field {
    name: "text",
    schema: Schema::Str {
        max_len: 200,
        chars: None,
    },
}];

fn main() {
    let notes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let add = {
        let notes = notes.clone();
        AgentTool {
            command: "notes.add".into(),
            description: "Add a note of up to 200 characters.".into(),
            args: Schema::Struct(TEXT_FIELDS),
            handler: Box::new(move |args: &Value| -> Result<Value, String> {
                let text = args["text"].as_str().unwrap_or_default().to_string();
                notes.borrow_mut().push(text);
                Ok(json!({ "count": notes.borrow().len() }))
            }),
        }
    };
    let clear = {
        let notes = notes.clone();
        AgentTool {
            command: "notes.clear".into(),
            description: "Delete every note. The user must confirm.".into(),
            args: Schema::Struct(&[]),
            handler: Box::new(move |_args: &Value| -> Result<Value, String> {
                let removed = notes.borrow_mut().drain(..).count();
                Ok(json!({ "removed": removed }))
            }),
        }
    };

    let policy = Policy {
        confirm: ["notes.clear".to_string()].into_iter().collect(),
        // A burst of three calls, then one more per second. The script uses no time, so the
        // fourth call is refused.
        rate: RateLimit {
            burst: 3,
            per_second: 1,
        },
        ..Policy::default()
    };
    let mut session = Session::new(vec![add, clear], policy).with_actor("agent:example");
    let kill = session.kill_switch();

    let script = [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"notes.add","arguments":{"text":"buy milk"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"notes.add","arguments":{"text":42}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"notes.clear","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"notes.add","arguments":{"text":"call back"}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"notes.add","arguments":{"text":"never"}}}"#,
    ];

    let mut declined = |name: &str, _args: &Value| {
        println!("  [confirm] {name}: the user was asked and said no");
        false
    };
    for (step, line) in script.iter().enumerate() {
        if step == 4 {
            // The kill switch is also set from a hotkey or the API. Here it is set by hand.
            kill.store(true, Ordering::SeqCst);
            println!("kill switch set");
        }
        println!("request {}", step + 1);
        let reply = session
            .handle_line(line, Duration::ZERO, &mut declined)
            .unwrap_or_default();
        println!("  reply: {reply}");
    }

    println!("--- action log ---");
    for record in &session.log {
        let outcome = match &record.outcome {
            Outcome::Ran => "ran".to_string(),
            Outcome::Failed(why) => format!("failed: {why}"),
            Outcome::Refused(why) => format!("refused: {why}"),
        };
        println!("{} -> {outcome}", record.command);
    }
    println!("notes kept: {:?}", notes.borrow());
}
