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
use tpt_shell_agent::catalog::{
    tools_from_catalog, CommandCatalog, CommandDescriptor, Dispatch, Runner,
};
use tpt_shell_agent::page;
use tpt_shell_agent::profile::{AgentConfig, RouteRules};
use tpt_shell_agent::record::{RecordedAction, Recorder};
use tpt_shell_agent::resources::PageTree;
use tpt_shell_agent::{wait_for_tool, AgentTool, Outcome as AgentOutcome, Session};
use tpt_shell_audit::{AuditLog, Entry, Outcome};
use tpt_shell_ipc::contract::Contract;
use tpt_shell_ipc::schema::{Field, Schema};
use tpt_shell_ipc::{Limits, Router};

use crate::agent_snapshot::{snapshot_source, PageScript};
use crate::record::Recording;

/// How often the UI thread looks for agent lines. Responses are not latency sensitive.
pub(crate) const TICK: Duration = Duration::from_millis(20);

/// What the app set up for its agent: the manifest's `[agent]` section, the profile to use, and
/// the tools the app offers.
pub(crate) struct AgentSetup {
    pub(crate) config: AgentConfig,
    pub(crate) profile: String,
    /// The typed commands (agent-flagged and hand-written). Each is offered only when the profile
    /// allows `invoke` and names it.
    pub(crate) tools: Vec<AgentTool>,
    /// The page actions `click`, `type_text` and `read`. Each is offered only when the profile
    /// lists its verb in `actions` (see [`page_verb`]). They are kept apart from `tools`, so a
    /// typed command cannot be mistaken for one.
    pub(crate) page_tools: Vec<AgentTool>,
    /// Runs the snapshot script in a window's page. When set, the session offers `wait_for`, if
    /// the profile allows it. The app sets it once the platform can run page scripts for it.
    pub(crate) page_script: Option<PageScript>,
    /// Runs the snapshot script for the page-tree resource. A second script over the same window,
    /// because each script is consumed by the tool it serves. When set, the session offers the page
    /// tree as a resource. The audit tail is not offered: it holds the parameters of every call.
    pub(crate) resource_script: Option<PageScript>,
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

/// Writes each call to the audit log, and to the app's recording when one is running. The audit
/// write comes first: if it fails the session stops, and the call is not recorded to the replay
/// either.
struct Fanout {
    audit: SharedAudit,
    replay: Option<Rc<Recording>>,
}

impl Recorder for Fanout {
    fn record(&mut self, action: &RecordedAction) -> Result<(), String> {
        self.audit.record(action)?;
        if let Some(replay) = &self.replay {
            let outcome = match action.outcome {
                AgentOutcome::Ran => "ran",
                AgentOutcome::Failed(_) => "failed",
                AgentOutcome::Refused(_) => "refused",
            };
            replay.agent_call(
                action.actor,
                action.command,
                action.args,
                outcome,
                action.duration,
            );
        }
        Ok(())
    }
}

/// Builds the session for the enabled profile, or `None` when the agent is off: the profile is
/// disabled or the `[agent]` section does not enable it. An agent that is on needs an audit log,
/// so that every call it makes is recorded. A running recording gets the calls too.
pub(crate) fn session(
    setup: AgentSetup,
    audit: Option<Rc<RefCell<AuditLog>>>,
    replay: Option<Rc<Recording>>,
) -> Result<Option<Session>, String> {
    let Some(profile) = setup.config.profile(&setup.profile).cloned() else {
        return Ok(None);
    };
    let Some(log) = audit else {
        return Err(
            "the agent needs an audit log: call App::audit or App::manifest_audit first".into(),
        );
    };
    // Typed commands need `invoke` and a listed name. Page actions need their own verb in
    // `actions`, so a profile that lists only `click` gets `click` and nothing else.
    let mut tools: Vec<AgentTool> = setup
        .tools
        .into_iter()
        .filter(|tool| profile.allows_command(&tool.command))
        .collect();
    tools.extend(
        setup
            .page_tools
            .into_iter()
            .filter(|tool| profile.allows_action(page_verb(&tool.command))),
    );
    if let Some(script) = setup.page_script {
        if profile.allows_action("wait_for") {
            tools.push(wait_for_tool(snapshot_source(script)));
        }
    }
    // The session's own allowlist is the same set as the filter above. Passing it again means a
    // tool the profile does not allow stays unlisted and uncallable even if the filter changes.
    let allowed: Vec<String> = tools.iter().map(|tool| tool.command.clone()).collect();
    let recorder = Fanout {
        audit: SharedAudit {
            log,
            started: Instant::now(),
        },
        replay,
    };
    // A tool name must be unique. The session answers a call with the first tool of that name, so a
    // typed command and a page action with the same name would hide one of them.
    let mut seen = std::collections::BTreeSet::new();
    for tool in &tools {
        if !seen.insert(tool.command.as_str()) {
            return Err(format!(
                "the agent tool name {:?} is used twice; rename one of the commands",
                tool.command
            ));
        }
    }
    // The session's own `invoke` gate would also stop the page actions, so it is open here. Each
    // tool was filtered above: a typed command still needs `invoke` through `allows_command`.
    let mut policy = profile.policy();
    policy.invoke = true;
    let mut session = Session::new(tools, policy)
        .with_commands(allowed)
        .with_recorder(Box::new(recorder))
        .with_actor(format!("agent:{}", profile.name));
    if let Some(script) = setup.resource_script {
        session = session.with_resources(Box::new(PageTree::new(snapshot_source(script))));
    }
    Ok(Some(session))
}

/// The arguments of an action on one snapshot element: its reference number.
const REF_ARGS: &[Field] = &[Field {
    name: "ref",
    schema: Schema::Int {
        min: 1,
        max: u32::MAX as i64,
    },
}];

/// The arguments of typing into one element: its reference number and the text.
const TYPE_ARGS: &[Field] = &[
    Field {
        name: "ref",
        schema: Schema::Int {
            min: 1,
            max: u32::MAX as i64,
        },
    },
    Field {
        name: "text",
        schema: Schema::Str {
            max_len: 2000,
            chars: None,
        },
    },
];

/// The reference an action names. A reference is one from the latest snapshot; the page refuses a
/// stale one.
fn reference_of(args: &Value) -> Result<u32, String> {
    args["ref"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| "ref must be a reference number from the page snapshot".to_string())
}

/// One tool that performs an action on a snapshot element. The page runs the action, and its
/// answer is the tool's result, or its error.
fn action_tool(
    command: &str,
    description: &str,
    args: Schema,
    script: PageScript,
    build: fn(&Value) -> Result<page::Action, String>,
) -> AgentTool {
    let mut script = script;
    AgentTool {
        command: command.into(),
        description: description.into(),
        args,
        handler: Box::new(move |args: &Value| {
            let action = build(args)?;
            let json = script(&page::action_script(&action))?
                .ok_or_else(|| "the page gave no result for the action".to_string())?;
            page::parse_action_result(&json)
        }),
    }
}

/// The verb in a profile's `actions` list that allows a page tool. Most tools are named after their
/// verb; `type_text` is the `type` verb.
fn page_verb(tool: &str) -> &str {
    match tool {
        "type_text" => "type",
        other => other,
    }
}

/// The click tool. A click on a link, or on a control that submits a form, goes where the page
/// says the click would go. The destination is checked against the profile's routes before the
/// click runs, so a click cannot take the agent out of the app's routes. An element that does not
/// navigate is clicked as before. Empty routes allow no navigation at all.
fn click_tool(make_script: &dyn Fn() -> PageScript, routes: RouteRules) -> AgentTool {
    let mut destination_script = make_script();
    let mut click_script = make_script();
    AgentTool {
        command: "click".into(),
        description: "Click an element of the page, by its reference from the page snapshot. A link or submit button may only lead to an address the profile allows.".into(),
        args: Schema::Struct(REF_ARGS),
        handler: Box::new(move |args: &Value| {
            let ref_id = reference_of(args)?;
            let destination = run_action(&mut destination_script, &page::Action::Destination { ref_id })?;
            if let Some(url) = destination.as_str() {
                if !routes.allows(url) {
                    return Err(format!(
                        "the click would open {url}, which the profile's routes do not allow"
                    ));
                }
            }
            run_action(&mut click_script, &page::Action::Click { ref_id })
        }),
    }
}

/// Runs one page action through `script` and returns its result, or its error.
fn run_action(script: &mut PageScript, action: &page::Action) -> Result<Value, String> {
    let json = script(&page::action_script(action))?
        .ok_or_else(|| "the page gave no result for the action".to_string())?;
    page::parse_action_result(&json)
}

/// The page actions an agent may use as tools: `click`, `type_text` and `read`. Each tool gets its
/// own page script from `make_script`. The session offers only the ones the profile names, so a
/// profile that does not list them gets none of them.
pub(crate) fn page_action_tools(
    make_script: impl Fn() -> PageScript,
    routes: RouteRules,
) -> Vec<AgentTool> {
    vec![
        click_tool(&make_script, routes),
        action_tool(
            "type_text",
            "Type text into an element of the page, by its reference from the page snapshot.",
            Schema::Struct(TYPE_ARGS),
            make_script(),
            |args| {
                Ok(page::Action::Type {
                    ref_id: reference_of(args)?,
                    text: args["text"]
                        .as_str()
                        .ok_or_else(|| "text is needed".to_string())?
                        .to_string(),
                })
            },
        ),
        action_tool(
            "read",
            "Read the text of an element of the page, by its reference from the page snapshot.",
            Schema::Struct(REF_ARGS),
            make_script(),
            |args| {
                Ok(page::Action::Read {
                    ref_id: reference_of(args)?,
                })
            },
        ),
    ]
}

/// The app's contract, read as the agent's catalog: one descriptor per command, flagged by the
/// contract's `agent` flag.
struct ContractCatalog<'a>(&'a Contract);

impl CommandCatalog for ContractCatalog<'_> {
    fn commands(&self) -> Vec<CommandDescriptor> {
        self.0
            .commands()
            .map(|command| CommandDescriptor {
                name: command.name.to_string(),
                description: format!("Calls the app's `{}` command.", command.name),
                args: command.args.clone(),
                // The agent holds no grants, so a command that needs a capability is never offered
                // to it. Only an unguarded command the contract flags is.
                agent: command.capability.is_none()
                    && self.0.flags(command.name).is_some_and(|f| f.agent),
                // The contract has no confirmation flag, so no contract command asks first.
                confirm: false,
            })
            .collect()
    }
}

/// Runs an agent call through the app's router. The router's unauthorized entry point runs a
/// command that declares no capability, and refuses one that needs a grant, so the agent runs the
/// open commands and nothing else (fail closed). The reply's data is the result; a refused or
/// failed call's message is what the agent sees.
fn contract_runner(router: Rc<Router>) -> Runner {
    Box::new(move |name: &str, args: &Value| {
        let request = serde_json::json!({ "v": 1, "id": 0, "cmd": name, "args": args }).to_string();
        let reply: Value =
            serde_json::from_str(&router.handle(request.as_bytes(), &Limits::default()))
                .map_err(|e| e.to_string())?;
        if reply["ok"] == Value::Bool(true) {
            Ok(reply["data"].clone())
        } else {
            Err(reply["error"]["message"]
                .as_str()
                .unwrap_or("the command failed")
                .to_string())
        }
    })
}

/// The tools the app's contract adds for the agent, beside the ones the app listed by hand. A
/// listed tool keeps its place: for a name both have, the hand-written tool wins.
pub(crate) fn contract_tools(
    contract: &Contract,
    listed: &[AgentTool],
    router: Rc<Router>,
) -> Vec<AgentTool> {
    let dispatch: Dispatch = Rc::new(RefCell::new(contract_runner(router)));
    tools_from_catalog(&ContractCatalog(contract), None, dispatch)
        .tools
        .into_iter()
        .filter(|tool| !listed.iter().any(|l| l.command == tool.command))
        .collect()
}

/// Starts reading agent lines from stdin and returns the tick that answers them on the UI thread.
/// The tick runs every [`TICK`]. Confirmation is always refused here: no native dialog is wired
/// to the agent yet, so a command the profile marks for confirmation does not run.
#[cfg(not(feature = "mcp-http"))]
pub(crate) fn stdio_tick(session: Session) -> impl FnMut() + 'static {
    stdio_and_http_tick(session, ())
}

/// Like the stdio tick, and also answers the MCP requests the HTTP bridge has queued. Both
/// transports share the one session, which only the UI thread touches.
#[cfg(feature = "mcp-http")]
pub(crate) fn stdio_tick(
    session: Session,
    http: Option<tpt_shell_agent::http::Bridge>,
) -> impl FnMut() + 'static {
    stdio_and_http_tick(session, http)
}

/// The shared tick. `http` is the bridge when the HTTP transport is on; `()` otherwise.
fn stdio_and_http_tick<H: HttpAnswers + 'static>(
    mut session: Session,
    http: H,
) -> impl FnMut() + 'static {
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
        http.answer(&mut session, start, &mut refuse);
    }
}

/// Answers queued HTTP requests on the UI thread. Implemented by the bridge, and by `()` for an
/// app without the HTTP transport.
trait HttpAnswers {
    fn answer(
        &self,
        session: &mut Session,
        start: Instant,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    );
}

impl HttpAnswers for () {
    fn answer(&self, _: &mut Session, _: Instant, _: &mut dyn FnMut(&str, &Value) -> bool) {}
}

#[cfg(feature = "mcp-http")]
impl HttpAnswers for Option<tpt_shell_agent::http::Bridge> {
    fn answer(
        &self,
        session: &mut Session,
        start: Instant,
        confirm: &mut dyn FnMut(&str, &Value) -> bool,
    ) {
        if let Some(bridge) = self {
            bridge.answer_pending(session, start, confirm);
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
            page_tools: Vec::new(),
            page_script: None,
            resource_script: None,
        }
    }

    /// A page script for a page whose one button is always there.
    fn page_with_save() -> PageScript {
        let page = r#"{"title":"t","path":"https://tpt-app.localhost/","truncated":false,"tree":[{"ref":7,"role":"button","name":"Save","bounds":{"x":0,"y":0,"w":1,"h":1}}]}"#;
        Box::new(move |_| Ok(Some(page.to_string())))
    }

    /// A page script that answers the destination query with `destination` and records each script.
    fn link_page(
        destination: Option<&'static str>,
        sent: Rc<RefCell<Vec<String>>>,
    ) -> impl Fn() -> PageScript {
        move || {
            let sent = sent.clone();
            Box::new(move |script: &str| {
                sent.borrow_mut().push(script.to_string());
                let data = if script.contains(r#""op":"destination""#) {
                    destination.map_or("null".to_string(), |url| format!("\"{url}\""))
                } else {
                    "\"done\"".to_string()
                };
                Ok(Some(format!(r#"{{"ok":true,"data":{data}}}"#)))
            })
        }
    }

    #[test]
    fn a_click_on_a_link_outside_the_routes_is_refused_and_never_sent() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let routes = RouteRules::new(vec!["app://tpt-app.localhost/*".into()]).expect("routes");
        let mut tools = page_action_tools(
            link_page(Some("https://example.com/"), sent.clone()),
            routes,
        );
        let err = (tools[0].handler)(&serde_json::json!({ "ref": 3 })).expect_err("refused");
        assert!(err.contains("https://example.com/"), "{err}");
        assert!(
            sent.borrow().iter().all(|s| !s.contains(r#""op":"click""#)),
            "the click must not run: {:?}",
            sent.borrow()
        );
    }

    #[test]
    fn a_click_on_a_link_inside_the_routes_runs() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let routes = RouteRules::new(vec!["app://tpt-app.localhost/*".into()]).expect("routes");
        let mut tools = page_action_tools(
            link_page(Some("app://tpt-app.localhost/help.html"), sent.clone()),
            routes,
        );
        assert_eq!(
            (tools[0].handler)(&serde_json::json!({ "ref": 3 })),
            Ok(serde_json::json!("done"))
        );
        assert!(sent.borrow().iter().any(|s| s.contains(r#""op":"click""#)));
    }

    #[test]
    fn the_page_action_tools_send_their_action_to_the_page_and_return_its_answer() {
        let sent: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let mut tools = page_action_tools(
            || {
                let sent = sent.clone();
                Box::new(move |script: &str| {
                    sent.borrow_mut().push(script.to_string());
                    // A click asks first where it would go. This button does not navigate.
                    if script.contains(r#""op":"destination""#) {
                        return Ok(Some(r#"{"ok":true,"data":null}"#.to_string()));
                    }
                    Ok(Some(r#"{"ok":true,"data":"done"}"#.to_string()))
                })
            },
            RouteRules::default(),
        );
        let names: Vec<&str> = tools.iter().map(|t| t.command.as_str()).collect();
        assert_eq!(names, ["click", "type_text", "read"]);
        assert_eq!(
            (tools[0].handler)(&serde_json::json!({ "ref": 3 })),
            Ok(serde_json::json!("done"))
        );
        assert_eq!(
            (tools[1].handler)(&serde_json::json!({ "ref": 3, "text": "hi" })),
            Ok(serde_json::json!("done"))
        );
        // A call without a reference never reaches the page.
        assert!((tools[0].handler)(&serde_json::json!({})).is_err());
        let scripts = sent.borrow();
        assert_eq!(
            scripts.len(),
            3,
            "the click's destination check, its click, then the type"
        );
        assert!(
            scripts[0].contains(r#"{"op":"destination","ref":3}"#),
            "{}",
            scripts[0]
        );
        assert!(
            scripts[1].contains(r#"{"op":"click","ref":3}"#),
            "{}",
            scripts[1]
        );
        assert!(scripts[2].contains(r#""op":"type""#), "{}", scripts[2]);
    }

    #[test]
    fn the_contract_flags_become_agent_tools_that_run_as_the_agent_window() {
        use tpt_shell_ipc::contract::{Command, Flags, Kind};
        use tpt_shell_ipc::schema::Schema;
        use tpt_shell_ipc::Target;

        let no_args = Schema::Struct(&[]);
        let flagged = Flags {
            ui: false,
            agent: true,
        };
        let mut contract = Contract::new();
        contract
            .add_flagged(
                Command {
                    name: "echo",
                    kind: Kind::Sync,
                    capability: None,
                    args: no_args.clone(),
                },
                flagged,
            )
            .expect("echo");
        contract
            .add_flagged(
                Command {
                    name: "wipe",
                    kind: Kind::Sync,
                    capability: Some("fs.write"),
                    args: no_args.clone(),
                },
                flagged,
            )
            .expect("wipe");
        contract
            .add_flagged(
                Command {
                    name: "internal",
                    kind: Kind::Sync,
                    capability: None,
                    args: no_args,
                },
                Flags {
                    ui: false,
                    agent: false,
                },
            )
            .expect("internal");

        let mut router = Router::new();
        router.register("echo", |args| Ok(args.clone()));
        router.register_guarded(
            "wipe",
            "fs.write",
            |_| Target::None,
            |_| Ok(serde_json::json!("wiped")),
        );
        let mut tools = contract_tools(&contract, &[], Rc::new(router));

        // Only the unguarded, flagged commands become tools, in the contract's order.
        let names: Vec<&str> = tools.iter().map(|t| t.command.as_str()).collect();
        assert_eq!(names, ["echo"]);
        // An unguarded command runs through the router and returns its data.
        assert_eq!(
            (tools[0].handler)(&serde_json::json!({ "x": 1 })),
            Ok(serde_json::json!({ "x": 1 }))
        );
        // The guarded command is not offered at all, so a list-then-call cannot reach it.
        assert_eq!(tools.len(), 1);
    }

    #[test]
    fn the_page_tree_is_a_resource_only_when_a_page_script_is_given() {
        let text = "[app]\nname = \"demo\"\n[agent]\nenabled = true\n\
             [agent.profiles.assistant]\nactions = [\"invoke\"]\ncommands = [\"wait_for\"]\n";
        let parsed = tpt_shell_manifest::parse(text).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let build = |resource_script: Option<PageScript>| AgentSetup {
            config: config.clone(),
            profile: "assistant".into(),
            tools: Vec::new(),
            page_tools: Vec::new(),
            page_script: None,
            resource_script,
        };
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#;

        let path = temp_log("resources");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, tpt_shell_audit::Policy::default()).expect("log"),
        ));
        let mut without = session(build(None), Some(log.clone()), None)
            .expect("session")
            .expect("enabled");
        let reply = without
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(!reply.contains("page tree"), "{reply}");

        let mut with = session(build(Some(Box::new(|_| Ok(None)))), Some(log), None)
            .expect("session")
            .expect("enabled");
        let reply = with
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("page tree"), "{reply}");
        assert!(
            !reply.contains("audit"),
            "the audit tail is not offered: {reply}"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn wait_for_is_offered_only_when_the_profile_allows_it_and_a_page_script_is_given() {
        let text = "[app]\nname = \"demo\"\n[agent]\nenabled = true\n\
             [agent.profiles.assistant]\nactions = [\"wait_for\"]\n";
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
            page_tools: Vec::new(),
            page_script: script,
            resource_script: None,
        };
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

        let path = temp_log("wait");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, tpt_shell_audit::Policy::default()).expect("log"),
        ));
        let mut offered = session(setup_with(Some(page_with_save())), Some(log.clone()), None)
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

        let mut without = session(setup_with(None), Some(log), None)
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
        let session = session(setup(false), None, None).expect("no error");
        assert!(session.is_none());
    }

    #[test]
    fn an_enabled_agent_needs_an_audit_log() {
        let err = session(setup(true), None, None).err().expect("refused");
        assert!(err.contains("audit log"), "{err}");
    }

    #[test]
    fn an_agent_call_is_run_and_written_to_the_shared_audit_log() {
        let path = temp_log("call");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let mut session = session(setup(true), Some(log.clone()), None)
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
    fn a_password_argument_is_not_written_to_the_audit_log() {
        let path = temp_log("secret");
        let policy = Policy {
            store_params: true,
            redact_keys: vec!["password".into()],
        };
        let log = Rc::new(RefCell::new(AuditLog::open(&path, policy).expect("log")));
        const PASSWORD_ARGS: &[Field] = &[Field {
            name: "password",
            schema: Schema::Str {
                max_len: 64,
                chars: None,
            },
        }];
        let mut setup = setup(true);
        setup.tools = vec![AgentTool {
            command: "set_volume".into(),
            description: String::new(),
            args: Schema::Struct(PASSWORD_ARGS),
            handler: Box::new(|_| Ok(serde_json::json!({ "done": true }))),
        }];
        let mut session = session(setup, Some(log), None)
            .expect("session")
            .expect("enabled");
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_volume","arguments":{"password":"hunter2-secret"}}}"#;
        let reply = session
            .handle_line(line, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("done"), "{reply}");

        let text = std::fs::read_to_string(&path).expect("audit file");
        assert!(
            !text.contains("hunter2-secret"),
            "the secret reached the log"
        );
        assert!(text.contains("[redacted]"), "{text}");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn an_agent_call_is_also_written_to_a_running_recording() {
        let path = temp_log("replay");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let recording_path = path.parent().expect("dir").join("run.tptreplay");
        let recording = Rc::new(crate::record::Recording::new(&recording_path, "demo"));
        let mut session = session(setup(true), Some(log), Some(recording.clone()))
            .expect("session")
            .expect("enabled");
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"set_volume","arguments":{}}}"#;
        let _ = session.handle_line(line, Duration::ZERO, &mut |_, _| false);
        recording.finish().expect("finish");

        let bytes = std::fs::read(&recording_path).expect("recording");
        let replay = tpt_shell_replay::read_compressed(&bytes, tpt_shell_replay::Mode::Strict)
            .expect("read");
        let calls: Vec<_> = replay
            .events
            .iter()
            .filter(|e| e.kind == "agent_call")
            .collect();
        assert_eq!(calls.len(), 1, "the call is in the recording once");
        assert_eq!(calls[0].data["command"], "set_volume");
        assert_eq!(calls[0].data["outcome"], "ran");
        assert_eq!(calls[0].data["actor"], "agent:assistant");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn the_session_lists_only_the_tools_the_profile_allows() {
        let path = temp_log("list");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let mut session = session(setup(true), Some(log), None)
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
    fn a_typed_command_cannot_share_a_name_with_a_page_action() {
        let path = temp_log("dup-names");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        let text = "[app]
name = \"demo\"
[agent]
enabled = true
             [agent.profiles.assistant]
actions = [\"click\", \"invoke\"]
";
        let parsed = tpt_shell_manifest::parse(text).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let setup = AgentSetup {
            config,
            profile: "assistant".into(),
            tools: vec![AgentTool {
                command: "click".into(),
                description: String::new(),
                args: Schema::Struct(&[]),
                handler: Box::new(|_| Ok(serde_json::json!({ "done": true }))),
            }],
            page_tools: page_action_tools(
                || Box::new(|_: &str| Ok(Some(r#"{"ok":true,"data":null}"#.to_string()))),
                RouteRules::default(),
            ),
            page_script: None,
            resource_script: None,
        };
        let err = match session(setup, Some(log), None) {
            Err(err) => err,
            Ok(_) => panic!("two tools named click should be refused"),
        };
        assert!(err.contains("\"click\" is used twice"), "{err}");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn a_page_action_is_offered_only_when_the_profile_lists_its_verb() {
        let path = temp_log("page-verbs");
        let log = Rc::new(RefCell::new(
            AuditLog::open(&path, Policy::default()).expect("log"),
        ));
        // Lists two page verbs and no `invoke`, so no typed command may be offered.
        let text = "[app]\nname = \"demo\"\n[agent]\nenabled = true\n\
             [agent.profiles.assistant]\nactions = [\"click\", \"read\"]\n";
        let parsed = tpt_shell_manifest::parse(text).expect("manifest");
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section).expect("section");
        let page_tools = page_action_tools(
            || Box::new(|_: &str| Ok(Some(r#"{"ok":true,"data":null}"#.to_string()))),
            RouteRules::default(),
        );
        let setup = AgentSetup {
            config,
            profile: "assistant".into(),
            tools: vec![AgentTool {
                command: "set_volume".into(),
                description: String::new(),
                args: Schema::Struct(&[]),
                handler: Box::new(|_| Ok(serde_json::json!({ "done": true }))),
            }],
            page_tools,
            page_script: None,
            resource_script: None,
        };
        let mut session = session(setup, Some(log), None)
            .expect("session")
            .expect("enabled");
        let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let reply = session
            .handle_line(list, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains(r#""name":"click""#), "{reply}");
        assert!(reply.contains(r#""name":"read""#), "{reply}");
        assert!(!reply.contains("type_text"), "type needs its verb: {reply}");
        assert!(
            !reply.contains("set_volume"),
            "no invoke, no commands: {reply}"
        );
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
