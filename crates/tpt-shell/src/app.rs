// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The app builder (spec §7.1). It gathers windows, grants and commands, then starts them.
//!
//! What runs today: Windows, with WebView2. Each window loads one of the app's assets from the
//! app origin, so its pages are at the origin its grants name. Its page messages go through the
//! capability-checked router with the window's own grants. A grant that needs consent asks the
//! user in a native dialog owned by the window; the default answer is No. On other platforms
//! [`App::run`] and [`App::build`] return [`Error::Unsupported`].

use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(windows)]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use crate::blob;
use tpt_shell_audit::Policy;
#[cfg(windows)]
use tpt_shell_audit::{AuditLog, Entry, Outcome};
use tpt_shell_capabilities::Grant;
use tpt_shell_core::backend::Asset;
#[cfg(windows)]
use tpt_shell_core::backend::WindowSpec;
#[cfg(windows)]
use tpt_shell_core::geometry::LogicalSize;
use tpt_shell_core::Error;
use tpt_shell_core::Result;
#[cfg(windows)]
use tpt_shell_ipc::Caller;
use tpt_shell_ipc::{Router, Target, Value};

pub use tpt_shell_capabilities::Scope;
pub use tpt_shell_ipc::CommandError;

/// A window the app opens at start.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    name: String,
    title: String,
    width: f64,
    height: f64,
    /// The asset the window opens, relative to the app origin.
    #[cfg_attr(not(windows), allow(dead_code))]
    page: String,
}

impl Window {
    /// A window called `name`. The name is the window's identity in grants.
    pub fn new(name: &str) -> Self {
        Window {
            name: name.to_string(),
            title: name.to_string(),
            width: 800.0,
            height: 600.0,
            page: "index.html".into(),
        }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    /// Size in logical pixels.
    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// The asset the window opens first, such as `index.html`. It is served from the app origin.
    pub fn page(mut self, path: &str) -> Self {
        self.page = path.to_string();
        self
    }

    #[cfg(windows)]
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            name: self.name.clone(),
            title: self.title.clone(),
            size: LogicalSize {
                width: self.width,
                height: self.height,
            },
            allow_navigation: Vec::new(),
        }
    }
}

/// Registers one guarded command on the router.
type Register = Box<dyn FnOnce(&mut Router)>;

/// Builds an app: its windows, grants and commands. Nothing starts until [`App::run`].
#[derive(Default)]
pub struct App {
    name: String,
    loader: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    windows: Vec<Window>,
    grants: Vec<Grant>,
    commands: Vec<Register>,
    /// Files the app serves from its origin, by path.
    #[cfg_attr(not(windows), allow(dead_code))]
    assets: HashMap<String, Asset>,
    /// Where command calls are audited, and what the log keeps.
    #[cfg_attr(not(windows), allow(dead_code))]
    audit: Option<(PathBuf, Policy)>,
    /// The single-instance id, and what to do with a later launch's arguments.
    #[cfg_attr(not(windows), allow(dead_code))]
    instance: Option<(String, ArgsHandler)>,
    /// The notification-area icon: tooltip, menu, and what a pick does.
    #[cfg_attr(not(windows), allow(dead_code))]
    tray: Option<(String, Vec<MenuItem>, TrayHandler)>,
}

/// Receives the arguments of a later launch of a single-instance app.
type ArgsHandler = Box<dyn FnMut(Vec<String>)>;

/// Receives the id of a tray click or menu pick.
type TrayHandler = Box<dyn FnMut(u32)>;

#[cfg(windows)]
pub use tpt_shell_platform_windows::tray::{MenuItem, ACTIVATE as TRAY_ACTIVATE};

impl App {
    pub fn new() -> Self {
        App {
            name: "tpt-app".into(),
            ..App::default()
        }
    }

    /// The app's name. It names the default data folder.
    pub fn name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// The WebView2 loader DLL (`WebView2Loader.dll`) to load. Required on Windows.
    pub fn loader(mut self, path: impl Into<PathBuf>) -> Self {
        self.loader = Some(path.into());
        self
    }

    /// Writes every command call from the app's pages to an audit log at `path` (spec §10.3).
    /// `policy` decides whether the parameters are kept; their hash is always kept.
    pub fn audit(mut self, path: impl Into<PathBuf>, policy: Policy) -> Self {
        self.audit = Some((path.into(), policy));
        self
    }

    /// Where WebView2 keeps its profile. Defaults to `%LOCALAPPDATA%\tpt-shell\<name>\webview2`.
    pub fn data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(path.into());
        self
    }

    pub fn window(mut self, window: Window) -> Self {
        self.windows.push(window);
        self
    }

    /// Adds a file the app serves from its origin, such as `index.html` or `app.js`. Pages load
    /// their scripts and styles from these, because the page policy blocks inline ones.
    pub fn asset(mut self, path: &str, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        self.assets.insert(
            path.to_string(),
            Asset {
                content_type: content_type.to_string(),
                body: body.into(),
            },
        );
        self
    }

    /// Adds several files at once, such as the output of `embed_dir!`.
    pub fn assets(mut self, files: &[(&str, &str, &[u8])]) -> Self {
        for (path, content_type, body) in files {
            self = self.asset(path, content_type, *body);
        }
        self
    }

    /// Runs `register` against the router. Use it for commands made with `#[command]`, for
    /// example `app.register(|router| register_commands!(router, list_dir))`.
    pub fn register(mut self, register: impl FnOnce(&mut Router) + 'static) -> Self {
        self.commands.push(Box::new(register));
        self
    }

    /// Adds a grant. It applies to the window and origin it names, and to no other.
    pub fn grant(mut self, grant: Grant) -> Self {
        self.grants.push(grant);
        self
    }

    /// Adds a command that pages may call. `capability` is what the caller must hold;
    /// `target_of` reads what the call acts on from its arguments, for the scope check.
    pub fn command(
        mut self,
        name: &str,
        capability: &str,
        target_of: impl Fn(&Value) -> Target + 'static,
        handler: impl Fn(&Value) -> std::result::Result<Value, CommandError> + 'static,
    ) -> Self {
        let name = name.to_string();
        let capability = capability.to_string();
        self.commands.push(Box::new(move |router: &mut Router| {
            router.register_guarded(&name, &capability, target_of, handler);
        }));
        self
    }

    /// Adds an icon to the notification area with a context menu (Windows). A left click calls
    /// `on_select` with [`TRAY_ACTIVATE`], and a right click shows the menu, whose pick calls it
    /// with that item's id (non-zero). The icon is removed when the app stops.
    pub fn tray(
        mut self,
        tooltip: &str,
        items: Vec<MenuItem>,
        on_select: impl FnMut(u32) + 'static,
    ) -> Self {
        self.tray = Some((tooltip.to_string(), items, Box::new(on_select)));
        self
    }

    /// Makes the app a single instance named `app_id` (Windows). A later launch does not open a
    /// second app: its arguments go to `on_forwarded` in the first one, and that launch's
    /// [`App::run`] returns at once. The first instance's arguments are not passed on.
    pub fn single_instance(
        mut self,
        app_id: &str,
        on_forwarded: impl FnMut(Vec<String>) + 'static,
    ) -> Self {
        self.instance = Some((app_id.to_string(), Box::new(on_forwarded)));
        self
    }

    /// Starts the app and runs it until its last window closes or the app quits.
    pub fn run(mut self) -> Result<()> {
        #[cfg(windows)]
        {
            let guard = match self.claim_instance()? {
                Claimed::Forwarded => return Ok(()),
                Claimed::Alone => None,
                Claimed::First(guard) => Some(guard),
            };
            self.build_with(guard)?.run()
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unsupported {
                feature: "running an app on this platform",
            })
        }
    }

    /// Creates the windows and wires them, without running the event loop. Windows only.
    /// Fails when another launch of a single-instance app already owns it.
    #[cfg(windows)]
    pub fn build(mut self) -> Result<Running> {
        let guard = match self.claim_instance()? {
            Claimed::Forwarded => {
                return Err(Error::Backend(
                    "another instance of this app is running and received these arguments".into(),
                ))
            }
            Claimed::Alone => None,
            Claimed::First(guard) => Some(guard),
        };
        self.build_with(guard)
    }

    /// Claims the single instance, if one was asked for.
    #[cfg(windows)]
    fn claim_instance(&mut self) -> Result<Claimed> {
        use tpt_shell_platform_windows::single_instance::{claim, Claim};

        let Some((app_id, on_forwarded)) = self.instance.take() else {
            return Ok(Claimed::Alone);
        };
        let args: Vec<String> = std::env::args().skip(1).collect();
        match claim(&app_id, &args).map_err(Error::Backend)? {
            Claim::First(mut guard) => {
                guard.on_forwarded(on_forwarded);
                Ok(Claimed::First(guard))
            }
            Claim::Forwarded => Ok(Claimed::Forwarded),
        }
    }

    /// Creates the windows, keeping `instance` alive for as long as the app runs.
    #[cfg(windows)]
    fn build_with(
        self,
        instance: Option<tpt_shell_platform_windows::single_instance::InstanceGuard>,
    ) -> Result<Running> {
        use std::cell::RefCell;
        use std::rc::Rc;
        use tpt_shell_capabilities::Engine;
        use tpt_shell_core::backend::{WebviewBackend, WindowBackend};
        use tpt_shell_ipc::{Caller, Limits};
        use tpt_shell_platform_windows::backend::WindowsBackend;
        use tpt_shell_platform_windows::consent::NativeConsent;
        use tpt_shell_platform_windows::webview2::{default_user_data_dir, APP_ORIGIN};

        let loader = self.loader.ok_or_else(|| {
            Error::Backend("App needs a WebView2 loader path (App::loader)".into())
        })?;
        let data_dir = match self.data_dir {
            Some(dir) => dir,
            None => {
                let local = std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .ok_or_else(|| Error::Backend("LOCALAPPDATA is not set".into()))?;
                default_user_data_dir(&local, &self.name).map_err(Error::Backend)?
            }
        };

        let tray = match self.tray {
            Some((tooltip, items, on_select)) => Some(
                tpt_shell_platform_windows::tray::Tray::new(&tooltip, items, on_select)
                    .map_err(Error::Backend)?,
            ),
            None => None,
        };

        let mut engine = Engine::new();
        for grant in self.grants {
            engine.add_grant(grant).map_err(|e| Error::Backend(e.0))?;
        }
        let engine = Rc::new(RefCell::new(engine));
        let mut router = Router::new();
        for register in self.commands {
            register(&mut router);
        }
        let router = Rc::new(router);
        let audit = match self.audit {
            Some((path, policy)) => {
                if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                    std::fs::create_dir_all(dir).map_err(|e| {
                        Error::Backend(format!("cannot create {}: {e}", dir.display()))
                    })?;
                }
                Some(Rc::new(RefCell::new(
                    AuditLog::open(&path, policy).map_err(|e| Error::Backend(e.to_string()))?,
                )))
            }
            None => None,
        };

        let assets = Rc::new(self.assets);
        let mut backend = WindowsBackend::new(&loader, &data_dir)?;
        let mut windows = Vec::new();
        for window in self.windows {
            let id = backend.create(&window.spec())?;
            backend.show(id)?;
            let caller = Caller {
                window: window.name.clone(),
                origin: APP_ORIGIN.into(),
            };
            let router = router.clone();
            let engine = engine.clone();
            let audit = audit.clone();
            backend.set_message_handler(
                id,
                Box::new(move |text| {
                    let mut prompt = NativeConsent::for_window(id);
                    let started = Instant::now();
                    let reply = router.handle_authorized(
                        &caller,
                        &mut engine.borrow_mut(),
                        &mut prompt,
                        text.as_bytes(),
                        &Limits::default(),
                    );
                    if let Some(log) = &audit {
                        let entry = audit_entry(&caller, &router, text, &reply, started.elapsed());
                        // The command has already run, so a failed write cannot undo it.
                        if let Err(e) = log.borrow_mut().append(&entry) {
                            eprintln!("tpt-shell: audit write failed: {e}");
                        }
                    }
                    Some(reply)
                }),
            )?;
            let assets = assets.clone();
            backend.set_asset_source(
                id,
                Box::new(move |path| match path.strip_prefix("__tpt/blob/") {
                    Some(blob_id) => blob::blob_asset(blob_id),
                    None => assets.get(path).cloned(),
                }),
            )?;
            backend.navigate(id, &format!("{APP_ORIGIN}/{}", window.page))?;
            windows.push((window.name, id));
        }
        Ok(Running {
            backend,
            windows,
            _instance: instance,
            _tray: tray,
        })
    }
}

/// Builds the audit record for one message from a page. The command and its arguments come from
/// the request, and the outcome from the reply. A message that does not parse is recorded as
/// `(unparsed)` with no parameters, so it still leaves a trace. The capability is the one the
/// command is registered behind, if any.
#[cfg(windows)]
fn audit_entry(
    caller: &Caller,
    router: &Router,
    text: &str,
    reply: &str,
    duration: Duration,
) -> Entry {
    let (command, params) = match serde_json::from_str::<Value>(text) {
        Ok(request) => (
            request["cmd"].as_str().unwrap_or("").to_string(),
            request["args"].clone(),
        ),
        Err(_) => ("(unparsed)".to_string(), Value::Null),
    };
    let capability = router.capability_of(&command).map(str::to_string);
    let outcome = match serde_json::from_str::<Value>(reply) {
        Ok(response) if response["ok"] == Value::Bool(true) => Outcome::Ok,
        Ok(response) => Outcome::Err(
            response["error"]["code"]
                .as_str()
                .unwrap_or("UNKNOWN")
                .to_string(),
        ),
        Err(_) => Outcome::Err("UNKNOWN".into()),
    };
    let wall_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    Entry {
        wall_unix_ms,
        mono_ns: process_start().elapsed().as_nanos() as u64,
        actor: caller.window.clone(),
        command,
        capability,
        params,
        outcome,
        duration_us: u64::try_from(duration.as_micros()).unwrap_or(u64::MAX),
    }
}

/// The reference point for `mono_ns`: the first time an audit record is built.
#[cfg(windows)]
fn process_start() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// A started app: its windows exist and their pages are loaded. Windows only.
#[cfg(windows)]
pub struct Running {
    backend: tpt_shell_platform_windows::backend::WindowsBackend,
    windows: Vec<(String, tpt_shell_platform_windows::backend::WindowId)>,
    /// Held so that no other launch can become the first instance while this app runs.
    _instance: Option<tpt_shell_platform_windows::single_instance::InstanceGuard>,
    /// Held so the icon stays until the app stops.
    _tray: Option<tpt_shell_platform_windows::tray::Tray>,
}

/// How a launch relates to a single-instance app.
#[cfg(windows)]
enum Claimed {
    /// The app did not ask for a single instance.
    Alone,
    /// This launch is the first instance.
    First(tpt_shell_platform_windows::single_instance::InstanceGuard),
    /// Another launch owns the app and received these arguments.
    Forwarded,
}

#[cfg(windows)]
impl Running {
    /// The backend, for reading titles and other window state.
    pub fn backend(&self) -> &tpt_shell_platform_windows::backend::WindowsBackend {
        &self.backend
    }

    /// The id of the window called `name`, if the app opened it.
    pub fn window(&self, name: &str) -> Option<tpt_shell_platform_windows::backend::WindowId> {
        self.windows
            .iter()
            .find(|(window, _)| window == name)
            .map(|(_, id)| *id)
    }

    /// Sends an event to the page of the window called `window`. The page gets it through
    /// `window.tpt.on(name, handler)`. Events are not queued: a page that is not listening
    /// yet misses them, so wait until the page has registered its handler.
    pub fn emit(&mut self, window: &str, name: &str, data: Value) -> Result<()> {
        use tpt_shell_core::backend::WebviewBackend;
        let id = self
            .window(window)
            .ok_or_else(|| Error::Backend(format!("no window named {window:?}")))?;
        let message = tpt_shell_ipc::event(name, data).to_string();
        self.backend.post_message(id, &message)
    }

    /// Runs the event loop until the app quits.
    pub fn run(mut self) -> Result<()> {
        self.backend.run_until_quit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn audit_entries_record_the_command_its_arguments_and_the_outcome() {
        let caller = Caller {
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
        };
        let mut router = Router::new();
        router.register_guarded("read", "fs.read", |_| Target::None, |_| Ok(Value::Null));
        router.register("greet", |_| Ok(Value::Null));
        let ok = audit_entry(
            &caller,
            &router,
            r#"{"v":1,"id":1,"cmd":"greet","args":{"name":"x"}}"#,
            r#"{"id":1,"ok":true,"data":1}"#,
            Duration::from_micros(7),
        );
        assert_eq!(ok.actor, "main");
        assert_eq!(ok.command, "greet");
        assert_eq!(ok.params, serde_json::json!({ "name": "x" }));
        assert_eq!(ok.outcome, Outcome::Ok);
        assert_eq!(ok.duration_us, 7);
        assert_eq!(ok.capability, None);

        let denied = audit_entry(
            &caller,
            &router,
            r#"{"v":1,"id":2,"cmd":"read","args":{}}"#,
            r#"{"id":2,"ok":false,"error":{"code":"CAP_DENIED","message":"no"}}"#,
            Duration::ZERO,
        );
        assert_eq!(denied.outcome, Outcome::Err("CAP_DENIED".into()));
        assert_eq!(denied.capability.as_deref(), Some("fs.read"));
    }

    #[cfg(windows)]
    #[test]
    fn a_later_launch_hands_its_arguments_over_and_does_not_start_the_app() {
        use std::sync::{Arc, Mutex};
        use tpt_shell_platform_windows::single_instance::{claim, Claim};

        let app_id = format!("app-test-{}", std::process::id());
        // Stands in for the first instance, which receives the arguments. This test thread is
        // the later launch.
        let mut first = match claim(&app_id, &[]).expect("first claim") {
            Claim::First(guard) => guard,
            Claim::Forwarded => panic!("the first claim should own the app"),
        };
        let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        first.on_forwarded(move |args| sink.lock().expect("sink").push(args));
        // No loader is set, so this only succeeds if `run` returns before it needs one.
        App::new()
            .single_instance(&app_id, |_| {})
            .run()
            .expect("a later launch returns at once");
        assert_eq!(seen.lock().expect("seen").len(), 1);

        let err = App::new()
            .single_instance(&app_id, |_| {})
            .build()
            .err()
            .expect("build refuses a forwarded launch");
        assert!(err.to_string().contains("another instance"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn a_message_that_does_not_parse_is_still_recorded() {
        let caller = Caller {
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
        };
        let junk = audit_entry(
            &caller,
            &Router::new(),
            "not json",
            "also not json",
            Duration::ZERO,
        );
        assert_eq!(junk.command, "(unparsed)");
        assert_eq!(junk.params, Value::Null);
        assert_eq!(junk.outcome, Outcome::Err("UNKNOWN".into()));
    }

    #[cfg(windows)]
    #[test]
    fn a_window_keeps_its_name_title_and_size() {
        let w = Window::new("main").title("Demo").size(1100.0, 700.0);
        let spec = w.spec();
        assert_eq!(spec.name, "main");
        assert_eq!(spec.title, "Demo");
        assert_eq!((spec.size.width, spec.size.height), (1100.0, 700.0));
        assert!(spec.allow_navigation.is_empty());
    }

    #[test]
    fn a_window_opens_index_html_unless_told_otherwise() {
        assert_eq!(Window::new("main").page, "index.html");
        assert_eq!(Window::new("main").page("start.html").page, "start.html");
    }

    #[test]
    fn embedded_directories_become_assets() {
        let app = App::new().assets(crate::embed_dir!("../../fixtures/app"));
        assert_eq!(app.assets.len(), 2);
        assert_eq!(
            app.assets["index.html"].content_type,
            "text/html; charset=utf-8"
        );
        assert_eq!(
            app.assets["app.js"].content_type,
            "text/javascript; charset=utf-8"
        );
    }

    #[test]
    fn assets_are_kept_by_path_and_a_later_one_replaces_an_earlier_one() {
        let app = App::new()
            .asset("index.html", "text/html", "<p>one</p>")
            .asset("index.html", "text/html", "<p>two</p>");
        assert_eq!(app.assets.len(), 1);
        assert_eq!(app.assets["index.html"].body, b"<p>two</p>");
    }

    #[cfg(not(windows))]
    #[test]
    fn running_an_app_is_unsupported_off_windows() {
        let err = App::new().run().unwrap_err();
        assert!(matches!(err, Error::Unsupported { .. }));
    }

    #[cfg(windows)]
    #[test]
    fn building_an_app_needs_a_loader() {
        match App::new().build() {
            Err(Error::Backend(message)) => assert!(message.contains("loader")),
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("an app without a loader must not start"),
        }
    }
}
