// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Agent host: an MCP server over stdio that exposes three notes tools to an AI agent.
//!
//! The agent sends one JSON-RPC message per line on stdin (`initialize`, `tools/list`,
//! `tools/call`) and reads one response per line from stdout. The policy below sets the rules:
//! `notes.clear` needs the user's confirmation, calls are rate limited, and every call is
//! logged in the session. This template has no native dialog, so confirmations are refused.
//!
//! This host has no window. A window's page is not offered to the agent yet (see todo.md, M9).

use std::cell::RefCell;
use std::io::{self, Write};
use std::rc::Rc;
use std::time::Instant;

use serde_json::{json, Value};
use tpt_shell_agent::{confirm_with, serve, AgentTool, DenyAll, Policy, Session};
use tpt_shell_ipc::schema::{Field, Schema};

const ADD_FIELDS: &[Field] = &[Field {
    name: "text",
    schema: Schema::Str {
        max_len: 200,
        chars: None,
    },
}];

fn main() -> io::Result<()> {
    let notes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let add = {
        let notes = notes.clone();
        AgentTool {
            command: "notes.add".into(),
            description: "Add a note. The text is up to 200 characters.".into(),
            args: Schema::Struct(ADD_FIELDS),
            handler: Box::new(move |args: &Value| -> Result<Value, String> {
                let text = args["text"].as_str().ok_or("text is required")?.to_string();
                notes.borrow_mut().push(text);
                Ok(json!({ "count": notes.borrow().len() }))
            }),
        }
    };
    let list = {
        let notes = notes.clone();
        AgentTool {
            command: "notes.list".into(),
            description: "List the notes, oldest first.".into(),
            args: Schema::Struct(&[]),
            handler: Box::new(move |_args: &Value| -> Result<Value, String> {
                Ok(json!({ "notes": notes.borrow().clone() }))
            }),
        }
    };
    let clear = {
        let notes = notes.clone();
        AgentTool {
            command: "notes.clear".into(),
            description: "Delete every note. The user must confirm this.".into(),
            args: Schema::Struct(&[]),
            handler: Box::new(move |_args: &Value| -> Result<Value, String> {
                let removed = notes.borrow_mut().drain(..).count();
                Ok(json!({ "removed": removed }))
            }),
        }
    };

    let policy = Policy {
        confirm: ["notes.clear".to_string()].into_iter().collect(),
        ..Policy::default()
    };
    let mut session = Session::new(vec![add, list, clear], policy).with_actor("agent:notes");

    let mut denial = DenyAll;
    let mut confirm = confirm_with(&mut denial);
    let mut out = io::stdout().lock();
    serve(
        &mut session,
        io::stdin().lock(),
        &mut out,
        Instant::now(),
        &mut confirm,
    )?;
    out.flush()
}
