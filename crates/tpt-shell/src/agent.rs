// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The agent surface of an app (spec §10.4). A manifest `[agent]` profile enables it; the app
//! lists the tools an agent may call, and the MCP session runs over the app's own stdio. The
//! session's tools are not `Send`, so lines are read on a thread and handled on the UI thread by
//! the tick this module returns. Every handled call is written to the app's audit log.

use std::cell::RefCell;
use std::io::{BufRead, Write};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tpt_shell_agent::profile::AgentConfig;
use tpt_shell_agent::record::{RecordedAction, Recorder};
use tpt_shell_agent::{wait_for_tool, AgentTool, Outcome as AgentOutcome, Session};
use tpt_shell_audit::{AuditLog, Entry, Outcome};

use crate::agent_snapshot::{snapshot_source, PageScript};

/// How often the UI thread looks for agent lines. Responses are not latency sensitive.
pub(crate) const TICK: Duration = Duration::from_millis(20);

/// What the app set up for its agent: the manifest's `[agent]` section, the profile to use, and
/// the tools the app offers.
pub(crate) struct AgentSetup {
    pub(crate) config: AgentConfig,
    pub(crate) profile: String,
    pub(crate) tools: Vec<AgentTool>,
    /// Runs the snapshot script in a window's page. When set, the session offers `wait_for`, if
    /// the profile allows it. The app sets it once the platform can run page scripts for it.
    pub(crate) page_script: Option<PageScript>,
}

/// Writes agent calls to the audit log the app's own commands use.
struct SharedAudit {
    log: Rc<RefCell<AuditLog>>,
    started: Instant,
}

impl Recorder for SharedAudit {
    fn record(&mut self, action: &RecordedAction) -> Result<(), String> {
        let outcome = match action.outcome {
            AgentOutcome::Ran => Outcome::Ok,
            AgentOutcome::Failed(_) => Outcome::Err("AGENT_FAILED".into()),
            AgentOutcome::Refused(_) => Outcome::Err("AGENT_REFUSED".into()),
        };
        let entry = Entry {
            wall_unix_ms: action.wall_unix_ms,
            mono_ns: u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            actor: action.actor.to_string(),
            command: action.command.to_string(),
            capability: None,
            params: action.args.clone(),
            outcome,
            duration_us: u64::try_from(action.duration.as_micros()).unwrap_or(u64::MAX),
        };
        self.log
            .borrow_mut()
            .append(&entry)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Builds the session for the enabled profile, or `None` when the agent is off: the profile is
/// disabled or the `[agent]` section does not enable it. An agent that is on needs an audit log,
/// so that every call it makes is recorded.
pub(crate) fn session(
    setup: AgentSetup,
    audit: Option<Rc<RefCell<AuditLog>>>,
) -> Result<Option<Session>, String> {
    let Some(profile) = setup.config.profile(&setup.profile).cloned() else {
        return Ok(None);
    };
    let Some(log) = audit else {
        return Err(
            "the agent needs an audit log: call App::audit or App::manifest_audit first".into(),
        );
    };
    let mut offered = setup.tools;
    if let Some(script) = setup.page_script {
        offered.push(wait_for_tool(snapshot_source(script)));
    }
    let tools: Vec<AgentTool> = offered
        .into_iter()
        .filter(|tool| profile.allows_command(&tool.command))
        .collect();
    // The session's own allowlist is the same set as the filter above. Passing it again means a
    // tool the profile does not allow stays unlisted and uncallable even if the filter changes.
    let allowed: Vec<String> = tools.iter().map(|tool| tool.command.clone()).collect();
    let recorder = SharedAudit {
        log,
        started: Instant::now(),
    };
    Ok(Some(
        Session::new(tools, profile.policy())
            .with_commands(allowed)
            .with_recorder(Box::new(recorder))
            .with_actor(format!("agent:{}", profile.name)),
    ))
}

/// Starts reading agent lines from stdin and returns the tick that answers them on the UI thread.
/// The tick runs every [`TICK`]. Confirmation is always refused here: no native dialog is wired
/// to the agent yet, so a command the profile marks for confirmation does not run.
pub(crate) fn stdio_tick(mut session: Session) -> impl FnMut() + 'static {
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    move || {
        let mut refuse = |_: &str, _: &Value| false;
        while let Ok(line) = rx.try_recv() {
            if let Some(reply) = session.handle_line(&line, start.elapsed(), &mut refuse) {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{reply}");
                let _ = out.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_shell_audit::Policy;
    use tpt_shell_ipc::schema::Schema;

    fn manifest(enabled: bool) -> String {
        format!(
            "[app]\nname = \"demo\"\n[agent]\nenabled = {enabled}\n\
             [agent.profiles.assistant]\nactions = [\"invoke\"]\ncommands = [\"set_volume\"]\n"
        )
    }

    fn setup(enabled: bool) -> AgentSetup {
        let parsed = tpt_shell_manifest::parse(&manifest(enabled)).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let tool = |name: &str| AgentTool {
            command: name.into(),
            description: String::new(),
            args: Schema::Struct(&[]),
            handler: Box::new(|_| Ok(serde_json::json!({ "done": true }))),
        };
        AgentSetup {
            config,
            profile: "assistant".into(),
            tools: vec![tool("set_volume"), tool("format_disk")],
            page_script: None,
        }
    }

    /// A page script for a page whose one button is always there.
    fn page_with_save() -> PageScript {
        let page = r#"{"title":"t","path":"https://tpt-app.localhost/","truncated":false,"tree":[{"ref":7,"role":"button","name":"Save","bounds":{"x":0,"y":0,"w":1,"h":1}}]}"#;
        Box::new(move |_| Ok(Some(page.to_string())))
    }

    #[test]
    fn wait_for_is_offered_only_when_the_profile_allows_it_and_a_page_script_is_given() {
        let text = "[app]\nname = \"demo\"\n[agent]\nenabled = true\n\
             [agent.profiles.assistant]\nactions = [\"invoke\"]\ncommands = [\"wait_for\"]\n";
        let parsed = tpt_shell_manifest::parse(text).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let setup_with = |script: Option<PageScript>| AgentSetup {
            config: config.clone(),
            profile: "assistant".into(),
            tools: Vec::new(),
            page_script: script,
        };
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

        let path = temp_log("wait");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, tpt_shell_audit::Policy::default()).expect("log"),
        ));
        let mut offered = session(setup_with(Some(page_with_save())), Some(log.clone()))
            .expect("session")
            .expect("enabled");
        let reply = offered
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("wait_for"), "{reply}");

        let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"wait_for","arguments":{"role":"button","name":"Save","timeout_ms":1000,"poll_ms":10}}}"#;
        let found = offered
            .handle_line(call, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(found.contains("Save"), "{found}");
        assert!(!found.contains("isError\":true"), "{found}");

        let mut without = session(setup_with(None), Some(log))
            .expect("session")
            .expect("enabled");
        let reply = without
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(!reply.contains("wait_for"), "{reply}");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    fn temp_log(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tpt-app-agent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir.join("audit.jsonl")
    }

    #[test]
    fn a_disabled_agent_is_not_started() {
        let session = session(setup(false), None).expect("no error");
        assert!(session.is_none());
    }

    #[test]
    fn an_enabled_agent_needs_an_audit_log() {
        let err = session(setup(true), None).err().expect("refused");
        assert!(err.contains("audit log"), "{err}");
    }

    #[test]
    fn an_agent_call_is_run_and_written_to_the_shared_audit_log() {
        let path = temp_log("call");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let mut session = session(setup(true), Some(log.clone()))
            .expect("session")
            .expect("enabled");
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_volume","arguments":{}}}"#;
        let reply = session
            .handle_line(line, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("done"), "{reply}");
        assert_eq!(log.borrow().len(), 1, "the call is recorded once");

        // A tool the profile does not allow is not offered at all.
        let refused = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"format_disk","arguments":{}}}"#;
        let _ = session.handle_line(refused, Duration::ZERO, &mut |_, _| false);
        assert_eq!(log.borrow().len(), 2, "a refused call is recorded too");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn the_session_lists_only_the_tools_the_profile_allows() {
        let path = temp_log("list");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let mut session = session(setup(true), Some(log))
            .expect("session")
            .expect("enabled");
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let reply = session
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("set_volume"), "{reply}");
        assert!(!reply.contains("format_disk"), "{reply}");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn the_manifest_routes_reach_the_profile_policy() {
        let text = "[app]\nname = \"demo\"\n[agent]\nenabled = true\n\
             [agent.profiles.assistant]\nactions = [\"click\"]\n\
             routes = [\"app://tpt-app.localhost/help.html\"]\n";
        let parsed = tpt_shell_manifest::parse(text).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let policy = config.profile("assistant").expect("profile").policy();
        assert!(policy.routes.allows("app://tpt-app.localhost/help.html"));
        assert!(!policy.routes.allows("https://example.com/"));
    }
}
