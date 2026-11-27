// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Replays a recording through an app's public API, with no window (spec §10.5). Windows only,
//! because the replay uses the Windows app origin.

#![cfg(windows)]

use std::path::PathBuf;

use serde_json::{json, Value};
use tpt_shell::{App, CommandError, Grant, Scope, Target};
use tpt_shell_replay::{Header, Mode, Recorder};

const ORIGIN: &str = "https://tpt-app.localhost";

/// Writes a two-call recording of `double`: the request, then a response with the given data.
fn write_recording(name: &str, answer: Value) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tpt-replay-{name}-{}.tptreplay",
        std::process::id()
    ));
    let mut recorder = Recorder::new(Header {
        app: "replay-test".into(),
        platform: "windows".into(),
    });
    recorder
        .record(
            10,
            "ipc_request",
            json!({ "id": 1, "cmd": "double", "args": { "n": 4 } }),
        )
        .expect("request");
    recorder
        .record(
            20,
            "ipc_response",
            json!({ "id": 1, "ok": true, "data": answer, "duration_ns": 777 }),
        )
        .expect("response");
    std::fs::write(&path, recorder.finish()).expect("write");
    path
}

fn doubling_app() -> App {
    App::new()
        .grant(Grant {
            capability: "math".into(),
            window: "main".into(),
            origin: ORIGIN.into(),
            scope: Scope::Unrestricted,
            consent: false,
        })
        .command(
            "double",
            "math",
            |_| Target::None,
            |args| -> Result<Value, CommandError> {
                Ok(json!({ "n": args["n"].as_i64().unwrap_or(0) * 2 }))
            },
        )
}

#[test]
fn a_recording_of_the_app_replays_clean_without_a_window() {
    let path = write_recording("clean", json!({ "n": 8 }));
    let outcome = doubling_app()
        .replay_recording(&path, Mode::Strict)
        .expect("the answers match, so a strict replay passes");
    let _ = std::fs::remove_file(&path);
    assert_eq!(outcome.divergence, None);
}

#[test]
fn a_changed_answer_fails_a_strict_replay_and_is_reported_when_lenient() {
    let path = write_recording("changed", json!({ "n": 9 }));
    let strict = doubling_app().replay_recording(&path, Mode::Strict);
    assert!(
        strict.is_err(),
        "a strict replay stops at the changed answer"
    );

    let lenient = doubling_app()
        .replay_recording(&path, Mode::Lenient)
        .expect("a lenient replay completes");
    let _ = std::fs::remove_file(&path);
    let divergence = lenient.divergence.expect("the answer differs");
    assert_eq!(divergence.index, 1, "the response is the first difference");
}
