// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The app builder (spec §7.1). It gathers windows, grants and commands, then starts them.
//!
//! What runs today: Windows, with WebView2. Each window loads one of the app's assets from the
//! app origin, so its pages are at the origin its grants name. Its page messages go through the
//! capability-checked router with the window's own grants. A grant that needs consent asks the
//! user in a native dialog owned by the window; the default answer is No. On other platforms
//! [`App::run`] and [`App::build`] return [`Error::Unsupported`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(all(windows, feature = "admin-endpoint"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(windows, feature = "admin-endpoint"))]
use std::sync::Arc;
use std::time::Duration;
#[cfg(windows)]
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use crate::blob;
#[cfg(windows)]
use crate::{devtools, session, ui};
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
use tpt_shell_devtools::IpcEntry;
#[cfg(windows)]
use tpt_shell_ipc::Caller;
use tpt_shell_ipc::{Router, Target, Value};
#[cfg(windows)]
use tpt_shell_kiosk::idle::IdleTimer;
#[cfg(windows)]
use tpt_shell_kiosk::recovery::CrashRecovery;
#[cfg(windows)]
use tpt_shell_platform_windows::webview2::default_user_data_dir;

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
    /// The blob store limit, when the app sets one (see `blob_limit`).
    blob_limit: Option<usize>,
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
    /// What a signed kiosk config sets, if one was loaded.
    #[cfg_attr(not(windows), allow(dead_code))]
    kiosk: Option<KioskSettings>,
    /// Periodic saving of app state, if asked for.
    #[cfg_attr(not(windows), allow(dead_code))]
    snapshot: Option<SnapshotPlan>,
    /// Which profile the windows use, if not the default folder.
    #[cfg_attr(not(windows), allow(dead_code))]
    profile: Option<tpt_shell_profiles::Kind>,
    /// Where window positions are remembered between runs, if anywhere.
    #[cfg_attr(not(windows), allow(dead_code))]
    state_file: Option<PathBuf>,
    /// The URL scheme to register for this app, if any.
    #[cfg_attr(not(windows), allow(dead_code))]
    deep_link: Option<String>,
    /// The command contract that `write_contract` and `--tpt-write-contract` write out.
    contract: Option<tpt_shell_ipc::contract::Contract>,
    /// The `[agent]` config and the profile name the app selected (feature `agent`).
    #[cfg(feature = "agent")]
    agent_config: Option<(tpt_shell_agent::profile::AgentConfig, String)>,
    /// The tools the app offers an agent (feature `agent`).
    #[cfg(feature = "agent")]
    agent_tools: Vec<tpt_shell_agent::AgentTool>,
    /// The health endpoint's address and the longest heartbeat age it accepts (feature `health-endpoint`).
    #[cfg(feature = "health-endpoint")]
    health: Option<(std::net::SocketAddr, Duration)>,
    /// The admin endpoint's address and token (feature `admin-endpoint`).
    #[cfg(feature = "admin-endpoint")]
    admin: Option<(std::net::SocketAddr, String)>,
}

/// What a kiosk config sets: the page the app returns to, and how long it may sit idle first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KioskSettings {
    /// A path under the app origin, such as `index.html`.
    pub home: String,
    /// After this long with no page messages, the windows go back to `home`. `None` never resets.
    pub idle: Option<Duration>,
    /// The whole policy from the config. The window placement and the lock-down features are
    /// applied by the backends; the key-chord hook is installed here (Windows, best effort).
    pub policy: tpt_shell_kiosk::policy::KioskPolicy,
}

/// Periodic saving of app state, and its restore at the next start.
#[cfg_attr(not(windows), allow(dead_code))]
struct SnapshotPlan {
    path: PathBuf,
    every: Duration,
    capture: Box<dyn FnMut() -> serde_json::Value>,
    restore: Option<Box<dyn FnOnce(serde_json::Value)>>,
}

/// Receives the arguments of a later launch of a single-instance app.
type ArgsHandler = Box<dyn FnMut(Vec<String>)>;

/// Schedules the periodic save and restores the saved state, once, before the windows show.
#[cfg(windows)]
fn start_snapshots(
    backend: &mut tpt_shell_platform_windows::backend::WindowsBackend,
    plan: SnapshotPlan,
) -> Result<(PathBuf, SnapshotCapture)> {
    if let Some(restore) = plan.restore {
        match tpt_shell_kiosk::state::restore::<serde_json::Value>(&plan.path) {
            Ok(Some(value)) => restore(value),
            Ok(None) => {}
            Err(e) => eprintln!("tpt-shell: starting without the saved state: {e}"),
        }
    }
    let capture: SnapshotCapture = Rc::new(RefCell::new(plan.capture));
    let for_tick = capture.clone();
    let path = plan.path.clone();
    backend.every(plan.every, move || {
        let value = (for_tick.borrow_mut())();
        if let Err(e) = tpt_shell_kiosk::state::save(&path, &value) {
            eprintln!("tpt-shell: cannot save the app state: {e}");
        }
    })?;
    Ok((plan.path, capture))
}

/// The folder the windows keep their data in, and the ephemeral session guard when there is one.
/// An explicit data folder wins, then the profile, then the app's default folder.
#[cfg(windows)]
fn resolve_data_dir(
    data_dir: Option<&PathBuf>,
    profile: Option<&tpt_shell_profiles::Kind>,
    name: &str,
) -> Result<(PathBuf, Option<tpt_shell_profiles::Ephemeral>)> {
    if let Some(dir) = data_dir {
        return Ok((dir.clone(), None));
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Backend("LOCALAPPDATA is not set".into()))?;
    match profile {
        None => Ok((
            default_user_data_dir(&local, name).map_err(Error::Backend)?,
            None,
        )),
        Some(kind) => profile_data_dir(&local, name, kind),
    }
}

/// The consent answers for a window's data folder, kept in `consent.txt` there. Named profiles
/// keep their answers; an ephemeral session drops them with its folder. If the file cannot be
/// opened, the answers are kept in memory for this run, so the user is asked again next start.
#[cfg(windows)]
fn consent_store(data_dir: &Path) -> Box<dyn tpt_shell_capabilities::ConsentStore> {
    use tpt_shell_capabilities::{FileConsent, MemoryConsent};
    let _ = std::fs::create_dir_all(data_dir);
    match FileConsent::open(data_dir.join("consent.txt")) {
        Ok(store) => Box::new(store),
        Err(_) => Box::new(MemoryConsent::default()),
    }
}

/// The data folder for a profile under `local` (the user's local app data), and the guard for an
/// ephemeral session. Named profiles live under `tpt-shell\<app>\profiles`.
#[cfg_attr(not(windows), allow(dead_code))]
fn profile_data_dir(
    local: &Path,
    app: &str,
    kind: &tpt_shell_profiles::Kind,
) -> Result<(PathBuf, Option<tpt_shell_profiles::Ephemeral>)> {
    use tpt_shell_profiles::{profile_dir, Ephemeral, Kind};
    match kind {
        Kind::Ephemeral => {
            let session = Ephemeral::create(&std::env::temp_dir())
                .map_err(|e| Error::Backend(format!("cannot create the session folder: {e}")))?;
            Ok((session.path().to_path_buf(), Some(session)))
        }
        Kind::Named(_) | Kind::Default => {
            let base = local.join("tpt-shell").join(app).join("profiles");
            let dir = profile_dir(&base, kind).map_err(|e| Error::Backend(e.to_string()))?;
            std::fs::create_dir_all(&dir)
                .map_err(|e| Error::Backend(format!("cannot create {}: {e}", dir.display())))?;
            Ok((dir, None))
        }
    }
}

/// The path after `--tpt-write-contract` on the command line, if the flag is there.
fn contract_flag() -> Result<Option<PathBuf>> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg.to_str() == Some("--tpt-write-contract") {
            return args
                .next()
                .map(|path| Some(PathBuf::from(path)))
                .ok_or_else(|| Error::Backend("--tpt-write-contract needs a file path".into()));
        }
    }
    Ok(None)
}

/// Reads and validates the manifest at `manifest`.
#[cfg(feature = "agent")]
fn read_manifest(manifest: &Path) -> Result<tpt_shell_manifest::Manifest> {
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| Error::Backend(format!("cannot read {}: {e}", manifest.display())))?;
    tpt_shell_manifest::parse(&text).map_err(|e| Error::Backend(e.to_string()))
}

/// Reads a kiosk config's settings. `home` must be a plain path under the app origin.
fn kiosk_settings(config: &serde_json::Value) -> Result<KioskSettings> {
    // The policy parser (tpt-shell-kiosk) checks every key. Only home and idle are applied
    // here; the window placement and lock-down values are for the platform backends.
    let policy =
        tpt_shell_kiosk::policy::parse(config).map_err(|e| Error::Backend(e.to_string()))?;
    Ok(KioskSettings {
        home: policy.home.clone(),
        idle: policy.idle,
        policy,
    })
}

/// Reads the window placements saved by [`Running::save_window_state`]. Entries that do not parse
/// are skipped, and a file that is missing or not JSON gives no placements.
#[cfg(windows)]
fn load_window_state(
    path: &std::path::Path,
) -> HashMap<String, tpt_shell_platform_windows::backend::Placement> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text)
    else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(name, v)| {
            let placement = tpt_shell_platform_windows::backend::Placement {
                x: i32::try_from(v["x"].as_i64()?).ok()?,
                y: i32::try_from(v["y"].as_i64()?).ok()?,
                width: i32::try_from(v["width"].as_i64()?)
                    .ok()
                    .filter(|w| *w > 0)?,
                height: i32::try_from(v["height"].as_i64()?)
                    .ok()
                    .filter(|h| *h > 0)?,
                maximized: v["maximized"].as_bool().unwrap_or(false),
            };
            Some((name, placement))
        })
        .collect()
}

/// Receives the id of a tray click or menu pick.
#[cfg_attr(not(windows), allow(dead_code))]
type TrayHandler = Box<dyn FnMut(u32)>;

/// One item in the tray icon's context menu. The menu is shown on Windows only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
}

/// The id a left click on the tray icon reports. Same value as the Windows `ACTIVATE`.
pub const TRAY_ACTIVATE: u32 = 0;

#[cfg(windows)]
const _: () = assert!(TRAY_ACTIVATE == tpt_shell_platform_windows::tray::ACTIVATE);

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

    /// The WebView2 loader DLL (`WebView2Loader.dll`) to load. Optional on Windows: without it,
    /// the app looks for `WebView2Loader.dll` beside the program.
    pub fn loader(mut self, path: impl Into<PathBuf>) -> Self {
        self.loader = Some(path.into());
        self
    }

    /// Writes command calls to the audit log at `log`, with the storage and redaction rules from
    /// the `[audit]` section of the manifest at `manifest` (spec §10.3). A manifest without that
    /// section keeps only the hashes. Fails when the manifest cannot be read or parsed.
    pub fn manifest_audit(
        mut self,
        manifest: &std::path::Path,
        log: impl Into<PathBuf>,
    ) -> Result<Self> {
        let text = std::fs::read_to_string(manifest)
            .map_err(|e| Error::Backend(format!("cannot read {}: {e}", manifest.display())))?;
        let parsed = tpt_shell_manifest::parse(&text).map_err(|e| Error::Backend(e.to_string()))?;
        self.audit = Some((log.into(), Policy::from(&parsed.audit)));
        Ok(self)
    }

    /// Loads a kiosk config and checks its signature against `public_key` (hex). The config is a
    /// JSON file with `home` (a path under the app origin, default `index.html`) and
    /// `idle_seconds` (a whole number above 0, optional). Fails when the signature does not check
    /// out, the file is not JSON, or a setting is out of range.
    pub fn kiosk_config(
        mut self,
        config: &std::path::Path,
        signature: &std::path::Path,
        public_key: &str,
    ) -> Result<Self> {
        let value = tpt_shell_kiosk::config::load(config, signature, public_key)
            .map_err(|e| Error::Backend(e.to_string()))?;
        self.kiosk = Some(kiosk_settings(&value)?);
        Ok(self)
    }

    /// Saves what `capture` returns to `path` every `every`, and at the end of the event loop.
    /// At the next start, if the file is there, `restore` is called once with its value, before
    /// the windows show. A snapshot that cannot be read is reported and skipped.
    pub fn snapshot_state(
        mut self,
        path: impl Into<PathBuf>,
        every: Duration,
        capture: impl FnMut() -> serde_json::Value + 'static,
        restore: impl FnOnce(serde_json::Value) + 'static,
    ) -> Self {
        self.snapshot = Some(SnapshotPlan {
            path: path.into(),
            every,
            capture: Box::new(capture),
            restore: Some(Box::new(restore)),
        });
        self
    }

    /// Chooses the profile the windows use (spec §10.7). `Named` keeps data until it is deleted.
    /// `Ephemeral` uses a new folder that is wiped when the app stops. An explicit
    /// [`App::data_dir`] wins over the profile.
    pub fn profile(mut self, kind: tpt_shell_profiles::Kind) -> Self {
        self.profile = Some(kind);
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

    /// The most bytes of blobs (`blob::put_blob`) the app holds at once. Defaults to 64 MiB.
    /// Support: Partial. Windows only; other platforms have no blob channel yet.
    pub fn blob_limit(mut self, bytes: usize) -> Self {
        self.blob_limit = Some(bytes);
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

    /// Remembers each window's position, size and maximized state in the JSON file at `path`,
    /// keyed by window name (Windows). The file is read when the app starts and written when the
    /// app's event loop ends. A missing or unreadable file means the windows open where they
    /// would otherwise.
    pub fn remember_window_state(mut self, path: impl Into<PathBuf>) -> Self {
        self.state_file = Some(path.into());
        self
    }

    /// Registers the URL scheme `scheme` for the current user when the app starts, so links such
    /// as `scheme://...` start this app with the link as an argument (Windows). A running
    /// instance gets the link through [`App::single_instance`].
    pub fn deep_link(mut self, scheme: &str) -> Self {
        self.deep_link = Some(scheme.to_string());
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

    /// Sets the command contract, the JSON description of the app's commands that the CLI reads.
    /// Build it with `commands![...]` from the `#[command]` functions (see [`CommandContract`]).
    pub fn contract(mut self, contract: tpt_shell_ipc::contract::Contract) -> Self {
        self.contract = Some(contract);
        self
    }

    /// Writes the contract as JSON to `path`. Fails when the app has no contract (see
    /// [`App::contract`]). [`App::run`] calls this for `--tpt-write-contract <path>` and returns
    /// before any window opens.
    pub fn write_contract(&self, path: &Path) -> Result<()> {
        let Some(contract) = &self.contract else {
            return Err(Error::Backend(
                "the app has no command contract: call App::contract first".into(),
            ));
        };
        std::fs::write(path, contract.to_json_string())
            .map_err(|e| Error::Backend(format!("cannot write {}: {e}", path.display())))
    }

    /// Reads the `[agent]` section of the manifest at `manifest` and selects `profile` from it
    /// (feature `agent`). The agent runs only when the section enables it and the profile exists;
    /// otherwise the app starts without one. The agent needs an audit log (see [`App::audit`]).
    #[cfg(feature = "agent")]
    pub fn agent(mut self, manifest: &Path, profile: &str) -> Result<Self> {
        let parsed = read_manifest(manifest)?;
        let section = parsed
            .stubs
            .iter()
            .find(|(name, _)| name.as_str() == "agent")
            .map(|(_, table)| table);
        let config = tpt_shell_agent::profile::parse_agent_section(section)
            .map_err(|e| Error::Backend(e.to_string()))?;
        if !config.profiles.contains_key(profile) {
            return Err(Error::Backend(format!(
                "the manifest has no agent profile named {profile:?}"
            )));
        }
        self.agent_config = Some((config, profile.to_string()));
        Ok(self)
    }

    /// Adds a command an agent may call (feature `agent`). `args` is its argument schema. The
    /// handler runs on the UI thread with the arguments the agent sent; its `Err` text is what the
    /// agent sees. Only the tools the selected profile allows are offered.
    #[cfg(feature = "agent")]
    pub fn agent_tool(
        mut self,
        command: &str,
        description: &str,
        args: tpt_shell_ipc::schema::Schema,
        handler: impl FnMut(&Value) -> std::result::Result<Value, String> + 'static,
    ) -> Self {
        self.agent_tools.push(tpt_shell_agent::AgentTool {
            command: command.to_string(),
            description: description.to_string(),
            args,
            handler: Box::new(handler),
        });
        self
    }

    /// Serves the health check on `addr`, which must be loopback (feature `health-endpoint`). It
    /// reads the heartbeat file named by `TPT_HEARTBEAT_FILE` and reports unhealthy once the beat
    /// is older than `max_age`.
    #[cfg(feature = "health-endpoint")]
    pub fn health_endpoint(mut self, addr: std::net::SocketAddr, max_age: Duration) -> Self {
        self.health = Some((addr, max_age));
        self
    }

    /// Serves the admin endpoint on `addr`, which must be loopback (feature `admin-endpoint`).
    /// Requests must carry `token`, at least 24 characters. The endpoint reports the app's window
    /// names, and a reset sends every window back to the home page.
    #[cfg(feature = "admin-endpoint")]
    pub fn admin_endpoint(mut self, addr: std::net::SocketAddr, token: &str) -> Self {
        self.admin = Some((addr, token.to_string()));
        self
    }

    /// Starts the app and runs it until its last window closes or the app quits. With
    /// `--tpt-write-contract <path>` it writes the contract instead, and returns `Ok` before any
    /// window opens, so the process exits with 0.
    #[cfg_attr(not(windows), allow(unused_mut))]
    pub fn run(mut self) -> Result<()> {
        if let Some(path) = contract_flag()? {
            return self.write_contract(&path);
        }
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
        blob::set_limit(self.blob_limit.unwrap_or(blob::LIMIT));
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
        mut self,
        instance: Option<tpt_shell_platform_windows::single_instance::InstanceGuard>,
    ) -> Result<Running> {
        use std::cell::RefCell;
        use std::rc::Rc;
        use tpt_shell_capabilities::Engine;
        use tpt_shell_core::backend::{EventLoopBackend, WebviewBackend, WindowBackend};
        use tpt_shell_ipc::{Caller, Limits};
        use tpt_shell_kiosk::policy::Mode;
        use tpt_shell_platform_windows::backend::WindowsBackend;
        use tpt_shell_platform_windows::consent::NativeConsent;
        use tpt_shell_platform_windows::webview2::APP_ORIGIN;

        let loader = self
            .loader
            .take()
            .or_else(loader_beside_program)
            .ok_or_else(|| {
                Error::Backend(
                    "App needs a WebView2 loader path: call App::loader, or put WebView2Loader.dll beside the program"
                        .into(),
                )
            })?;
        let (data_dir, ephemeral) =
            resolve_data_dir(self.data_dir.as_ref(), self.profile.as_ref(), &self.name)?;

        // A single-app kiosk is one full-screen window that navigates only inside its allow-list.
        let single_app = self
            .kiosk
            .as_ref()
            .filter(|k| k.policy.mode == Mode::SingleApp)
            .map(|k| k.policy.clone());
        if let Some(policy) = &single_app {
            if policy.windows.single_window && self.windows.len() > 1 {
                return Err(Error::Backend(format!(
                    "a single-app kiosk opens one window, but this app declares {}",
                    self.windows.len()
                )));
            }
        }
        let fullscreen = single_app.as_ref().is_some_and(|p| p.windows.fullscreen);
        // Entries with a scheme are extra origins; path entries are already under the app origin.
        let kiosk_allow: Vec<String> = single_app
            .as_ref()
            .map(|p| {
                p.allow
                    .iter()
                    .filter(|entry| entry.contains("://"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        if let Some(scheme) = &self.deep_link {
            let exe = std::env::current_exe()
                .map_err(|e| Error::Backend(format!("cannot find this program: {e}")))?;
            tpt_shell_platform_windows::deep_link::register(scheme, &exe, &self.name)
                .map_err(Error::Backend)?;
        }
        let tray = match self.tray {
            Some((tooltip, items, on_select)) => {
                let items = items
                    .into_iter()
                    .map(|item| tpt_shell_platform_windows::tray::MenuItem {
                        id: item.id,
                        label: item.label,
                    })
                    .collect();
                Some(
                    tpt_shell_platform_windows::tray::Tray::new(&tooltip, items, on_select)
                        .map_err(Error::Backend)?,
                )
            }
            None => None,
        };

        // The developer overlay's state (debug builds only). It reads the grants and the contract,
        // so it is made before the grants are moved into the engine.
        let devtools_shell = tpt_shell_devtools::enabled().then(|| {
            Rc::new(devtools::Shell::new(
                self.audit.as_ref().map(|(path, _)| path.clone()),
                &self.grants,
                self.contract.as_ref(),
            ))
        });
        let mut engine = Engine::with_consent_store(consent_store(&data_dir));
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

        // The licence page is built in unless the app serves its own file at that path.
        let mut app_assets = std::mem::take(&mut self.assets);
        app_assets
            .entry(crate::licenses::LICENSES_PATH.to_string())
            .or_insert_with(crate::licenses::asset);
        let assets = Rc::new(app_assets);
        let mut backend = WindowsBackend::new(&loader, &data_dir)?;

        // The agent, when its manifest profile enables it. It writes to the same audit log.
        #[cfg(feature = "agent")]
        let agent_tick = match self.agent_config.take() {
            Some((config, profile)) => {
                let setup = crate::agent::AgentSetup {
                    config,
                    profile,
                    tools: std::mem::take(&mut self.agent_tools),
                };
                crate::agent::session(setup, audit.clone())
                    .map_err(Error::Backend)?
                    .map(crate::agent::stdio_tick)
            }
            None => None,
        };
        #[cfg(feature = "health-endpoint")]
        let health_server = match self.health.take() {
            Some((addr, max_age)) => Some(crate::endpoints::health(addr, max_age)?),
            None => None,
        };
        #[cfg(feature = "admin-endpoint")]
        let reset_home = Arc::new(AtomicBool::new(false));
        #[cfg(feature = "admin-endpoint")]
        let admin_server = match self.admin.take() {
            Some((addr, token)) => {
                let names: Vec<String> = self.windows.iter().map(|w| w.name.clone()).collect();
                let status = Arc::new(move || serde_json::json!({ "windows": names }).to_string());
                let flag = reset_home.clone();
                let reset = Arc::new(move || flag.store(true, Ordering::SeqCst));
                Some(crate::endpoints::admin(addr, &token, status, reset)?)
            }
            None => None,
        };
        // Virtual devices from TPT_VIRTUAL_DEVICES (see `tpt_shell_devices::virtual_devices`).
        let devices = match std::env::var("TPT_VIRTUAL_DEVICES") {
            Ok(spec) if !spec.trim().is_empty() => {
                let transports = tpt_shell_devices::virtual_devices::from_spec(&spec)
                    .map_err(|e| Error::Backend(format!("TPT_VIRTUAL_DEVICES: {e}")))?;
                Some(tpt_shell_devices::hub::DeviceHub::new(
                    transports,
                    |_: &tpt_shell_devices::hub::Event| {},
                ))
            }
            _ => None,
        };
        let kiosk = self.kiosk;
        // Single-app kiosks block the chords Alt+Tab, Win, Alt+F4 and Ctrl+Esc (best effort).
        let keys = match &kiosk {
            Some(settings)
                if settings.policy.mode == tpt_shell_kiosk::policy::Mode::SingleApp
                    && settings.policy.lockdown.key_chords =>
            {
                Some(
                    tpt_shell_kiosk::hook_windows::install()
                        .map_err(|e| Error::Backend(format!("cannot block the key chords: {e}")))?,
                )
            }
            _ => None,
        };
        let snapshot = self.snapshot;
        let state_file = self.state_file;
        // Any page message counts as activity for the idle reset.
        let idle = kiosk
            .as_ref()
            .and_then(|k| k.idle)
            .map(|limit| Rc::new(RefCell::new(IdleTimer::new(limit, Instant::now()))));
        let saved = state_file
            .as_deref()
            .map(load_window_state)
            .unwrap_or_default();
        // The developer overlay's window, when the build has one (debug builds). Created hidden.
        let overlay = match &devtools_shell {
            Some(shell) => devtools::create(&mut backend, shell)?.map(|id| (id, shell.clone())),
            None => None,
        };
        // The windows by id, with the URL the shell last loaded in each. The hooks use them.
        let panes = Rc::new(session::Panes::default());
        // The kiosk's home page as a full URL. Crash recovery goes there when it gives up.
        let kiosk_home = kiosk.as_ref().map(|k| format!("{APP_ORIGIN}/{}", k.home));
        backend.on_window_closed(session::close_hook(panes.clone()))?;
        let mut windows = Vec::new();
        for window in self.windows {
            let mut spec = window.spec();
            spec.allow_navigation = kiosk_allow.clone();
            let id = backend.create(&spec)?;
            panes.add(id, &window.name);
            if let Some(placement) = saved.get(&window.name) {
                if let Err(e) = backend.set_placement(id, *placement) {
                    eprintln!("tpt-shell: cannot restore window {}: {e}", window.name);
                }
            }
            backend.show(id)?;
            if fullscreen {
                backend.set_fullscreen(id, true)?;
            }
            let caller = Caller {
                window: window.name.clone(),
                origin: APP_ORIGIN.into(),
            };
            let router = router.clone();
            let engine = engine.clone();
            let audit = audit.clone();
            let idle = idle.clone();
            // Kiosk windows recover a page whose process failed. The page's messages mark the last
            // page the shell loaded as good.
            let crash = kiosk_home.as_ref().map(|home| {
                Rc::new(RefCell::new(CrashRecovery::new(
                    home.clone(),
                    session::CRASH_WINDOW,
                    session::CRASH_LIMIT,
                )))
            });
            if let (Some(recovery), Some(home)) = (&crash, &kiosk_home) {
                backend.on_process_failed(
                    id,
                    session::recover(panes.clone(), id, recovery.clone(), home.clone()),
                )?;
            }
            let panes_for_page = panes.clone();
            let overlay_for_page = overlay.clone();
            backend.set_message_handler(
                id,
                Box::new(move |text| {
                    if let Some(recovery) = &crash {
                        if let Some(url) = panes_for_page.url_of(id) {
                            recovery.borrow_mut().page_loaded(&url);
                        }
                    }
                    if let Some((overlay_id, shell)) = &overlay_for_page {
                        if text == devtools::HOTKEY_MESSAGE {
                            if let Err(e) = devtools::open(*overlay_id, shell) {
                                eprintln!("tpt-shell: cannot open the developer overlay: {e}");
                            }
                            return None;
                        }
                    }
                    let mut prompt = NativeConsent::for_window(id);
                    if let Some(idle) = &idle {
                        idle.borrow_mut().touch(Instant::now());
                    }
                    let started = Instant::now();
                    let reply = router.handle_authorized(
                        &caller,
                        &mut engine.borrow_mut(),
                        &mut prompt,
                        text.as_bytes(),
                        &Limits::default(),
                    );
                    // Built only when something keeps it: the audit log or the overlay's log.
                    let entry = (audit.is_some() || overlay_for_page.is_some())
                        .then(|| audit_entry(&caller, &router, text, &reply, started.elapsed()));
                    if let (Some((_, shell)), Some(entry)) = (&overlay_for_page, &entry) {
                        let outcome = match &entry.outcome {
                            Outcome::Ok => "ok".to_string(),
                            Outcome::Err(code) => code.clone(),
                        };
                        shell.record(IpcEntry {
                            command: entry.command.clone(),
                            latency_us: entry.duration_us,
                            outcome,
                        });
                    }
                    if let (Some(log), Some(entry)) = (&audit, &entry) {
                        // The command has already run, so a failed write cannot undo it.
                        if let Err(e) = log.borrow_mut().append(entry) {
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
            if overlay.is_some() {
                devtools::add_hotkey(&mut backend, id)?;
            }
            session::go(&panes, id, &format!("{APP_ORIGIN}/{}", window.page))?;
            windows.push((window.name, id));
        }
        let window_ids: Vec<_> = windows.iter().map(|(_, id)| *id).collect();
        if let (Some(settings), Some(idle)) = (&kiosk, idle) {
            // Ticks a few times per idle period, so a reset happens soon after the limit.
            let limit = settings.idle.unwrap_or_default();
            let period = (limit / 4).clamp(Duration::from_millis(50), Duration::from_secs(1));
            let home = format!("{APP_ORIGIN}/{}", settings.home);
            let panes = panes.clone();
            backend.every(period, move || {
                if idle.borrow().is_due(Instant::now()) {
                    // The reset clears the session (cookies, storage, cache), then goes home.
                    session::clear_and_go_home(panes.clone(), window_ids.clone(), home.clone());
                    idle.borrow_mut().touch(Instant::now());
                }
            })?;
        }
        // Under the supervisor (TPT_HEARTBEAT_FILE is set), beat once a second. A UI thread that
        // stops running stops the beats, and the supervisor then restarts the app.
        if let Some(path) = tpt_shell_kiosk::heartbeat::from_env() {
            backend.every(Duration::from_secs(1), move || {
                if let Err(e) = tpt_shell_kiosk::heartbeat::beat(&path) {
                    eprintln!("tpt-shell: cannot write the heartbeat: {e}");
                }
            })?;
        }
        #[cfg(feature = "agent")]
        if let Some(tick) = agent_tick {
            backend.every(crate::agent::TICK, tick)?;
        }
        // An admin reset sends every window back to the home page, kiosk or not.
        #[cfg(feature = "admin-endpoint")]
        {
            let home = format!(
                "{APP_ORIGIN}/{}",
                kiosk
                    .as_ref()
                    .map(|k| k.home.as_str())
                    .unwrap_or("index.html")
            );
            let ids: Vec<_> = windows.iter().map(|(_, id)| *id).collect();
            let panes = panes.clone();
            backend.every(Duration::from_millis(250), move || {
                if reset_home.swap(false, Ordering::SeqCst) {
                    for id in &ids {
                        if let Err(e) = session::go(&panes, *id, &home) {
                            eprintln!("tpt-shell: cannot reset to the home page: {e}");
                        }
                    }
                }
            })?;
        }
        let snapshot_state = match snapshot {
            Some(plan) => Some(start_snapshots(&mut backend, plan)?),
            None => None,
        };
        let ui = ui::UiHandle::new(backend.proxy(), windows.clone());
        Ok(Running {
            backend,
            windows,
            ui,
            _instance: instance,
            tray,
            state_file,
            snapshot_state,
            _ephemeral: ephemeral,
            _keys: keys,
            devices,
            #[cfg(feature = "health-endpoint")]
            _health: health_server,
            #[cfg(feature = "admin-endpoint")]
            _admin: admin_server,
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

/// `WebView2Loader.dll` in the folder of the running program, when it is there.
#[cfg(any(windows, test))]
fn loader_beside_program() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let path = dir.join("WebView2Loader.dll");
    path.is_file().then_some(path)
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
    /// The notification-area icon, which stays until the app stops.
    tray: Option<tpt_shell_platform_windows::tray::Tray>,
    /// Where window positions are saved when the event loop ends.
    state_file: Option<PathBuf>,
    /// The session folder of an ephemeral profile. Dropping the app wipes it.
    _ephemeral: Option<tpt_shell_profiles::Ephemeral>,
    /// The key-chord hook of a single-app kiosk. Dropping the app removes it.
    _keys: Option<tpt_shell_kiosk::hook_windows::KeyboardHook>,
    /// The state file and the capture, for the save at the end of the event loop.
    snapshot_state: Option<(PathBuf, SnapshotCapture)>,
    /// The virtual devices from `TPT_VIRTUAL_DEVICES`, if any.
    devices: Option<tpt_shell_devices::hub::DeviceHub>,
    /// The UI-thread handle other threads use (see `Running::ui_handle`).
    ui: ui::UiHandle,
    /// The health endpoint, which stops when the app does.
    #[cfg(feature = "health-endpoint")]
    _health: Option<tpt_shell_kiosk::http::Server>,
    /// The admin endpoint, which stops when the app does.
    #[cfg(feature = "admin-endpoint")]
    _admin: Option<tpt_shell_kiosk::http::Server>,
}

/// The capture function, shared by the periodic save and the save at exit.
#[cfg_attr(not(windows), allow(dead_code))]
type SnapshotCapture = Rc<RefCell<Box<dyn FnMut() -> serde_json::Value>>>;

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

    /// Shows a balloon notification from the tray icon. Needs an icon, so it fails when the app
    /// has no [`App::tray`].
    pub fn notify(&self, title: &str, body: &str) -> Result<()> {
        match &self.tray {
            Some(tray) => tray.notify(title, body).map_err(Error::Backend),
            None => Err(Error::Backend(
                "the app has no tray icon to notify from".into(),
            )),
        }
    }

    /// The virtual devices from `TPT_VIRTUAL_DEVICES`, if the variable was set. The hub is not yet
    /// reachable from app commands; it is available to code that holds the `Running`.
    pub fn devices(&mut self) -> Option<&mut tpt_shell_devices::hub::DeviceHub> {
        self.devices.as_mut()
    }

    /// A handle that other threads use to ask the UI thread to show a window, run a script in a
    /// window or send a window an event. Clone it and move it to any thread. Each call returns at
    /// once; the work runs on the UI thread.
    ///
    /// Support: Full on Windows.
    pub fn ui_handle(&self) -> ui::UiHandle {
        self.ui.clone()
    }

    /// Runs the event loop until the app quits.
    pub fn run(mut self) -> Result<()> {
        let result = self.backend.run_until_quit();
        // The windows are gone, so their blobs go with them. A window that closed while the app ran
        // had its blobs removed then, by the close hook (see `session::close_hook`).
        for (name, _) in &self.windows {
            blob::remove_window_blobs(name);
        }
        if let Some((path, capture)) = &self.snapshot_state {
            let value = (capture.borrow_mut())();
            if let Err(e) = tpt_shell_kiosk::state::save(path, &value) {
                eprintln!("tpt-shell: cannot save the app state: {e}");
            }
        }
        if self.state_file.is_some() {
            if let Err(e) = self.save_window_state() {
                eprintln!("tpt-shell: cannot save the window state: {e}");
            }
        }
        result
    }

    /// Writes each window's placement to the state file, if one was set with
    /// [`App::remember_window_state`]. Does nothing otherwise.
    pub fn save_window_state(&self) -> Result<()> {
        let Some(path) = &self.state_file else {
            return Ok(());
        };
        let mut map = serde_json::Map::new();
        for (name, id) in &self.windows {
            let p = self.backend.placement(*id)?;
            map.insert(
                name.clone(),
                serde_json::json!({
                    "x": p.x, "y": p.y, "width": p.width, "height": p.height,
                    "maximized": p.maximized,
                }),
            );
        }
        let text = serde_json::to_string_pretty(&map).map_err(|e| Error::Backend(e.to_string()))?;
        std::fs::write(path, text)
            .map_err(|e| Error::Backend(format!("cannot write {}: {e}", path.display())))
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

    #[test]
    fn a_signed_kiosk_config_sets_the_home_page_and_idle_limit() {
        let dir = std::env::temp_dir().join(format!("tpt-app-kiosk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let secret = [7u8; 32];
        let public = tpt_shell_kiosk::config::public_key_hex(&secret);
        let config = dir.join("kiosk.json");
        let sig = dir.join("kiosk.json.sig");
        let body = br#"{"home":"start.html","idle_seconds":90}"#;
        std::fs::write(&config, body).expect("write config");
        std::fs::write(&sig, tpt_shell_kiosk::config::sign(body, &secret)).expect("write sig");
        let app = App::new()
            .kiosk_config(&config, &sig, &public)
            .expect("config");
        let settings = app.kiosk.as_ref().expect("kiosk settings");
        assert_eq!(settings.home, "start.html");
        assert_eq!(settings.idle, Some(Duration::from_secs(90)));
        assert_eq!(
            settings.policy.mode,
            tpt_shell_kiosk::policy::Mode::SingleApp
        );

        // An edited config no longer matches its signature.
        std::fs::write(&config, br#"{"home":"x.html"}"#).expect("edit config");
        assert!(App::new().kiosk_config(&config, &sig, &public).is_err());

        // A home path that leaves the app is refused, even when it is signed.
        let escape = br#"{"home":"../secret.html"}"#;
        std::fs::write(&config, escape).expect("write escape");
        std::fs::write(&sig, tpt_shell_kiosk::config::sign(escape, &secret)).expect("sign");
        assert!(App::new().kiosk_config(&config, &sig, &public).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profiles_choose_their_folders_and_an_ephemeral_session_is_wiped_when_dropped() {
        use tpt_shell_profiles::Kind;
        let local = std::env::temp_dir().join(format!("tpt-app-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        std::fs::create_dir_all(&local).expect("local");

        let (named, guard) =
            profile_data_dir(&local, "demo", &Kind::Named("work".into())).expect("named");
        assert!(guard.is_none(), "a named profile is not temporary");
        assert!(
            named.is_dir() && named.starts_with(&local),
            "{}",
            named.display()
        );

        let (session, guard) = profile_data_dir(&local, "demo", &Kind::Ephemeral).expect("session");
        let guard = guard.expect("an ephemeral session has a guard");
        std::fs::write(session.join("cookie.db"), b"data").expect("write into the session");
        assert!(session.is_dir());
        drop(guard);
        assert!(!session.exists(), "the session folder was not wiped");
        let _ = std::fs::remove_dir_all(&local);
    }

    #[cfg(windows)]
    #[test]
    fn consent_answers_are_kept_in_the_profile_folder() {
        let dir = std::env::temp_dir().join(format!("tpt-app-consent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut store = consent_store(&dir);
            store.set("main", "device.serial", true);
        }
        let reopened = consent_store(&dir);
        assert_eq!(reopened.get("main", "device.serial"), Some(true));
        assert!(dir.join("consent.txt").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_audit_section_of_the_manifest_sets_the_log_policy() {
        let dir = std::env::temp_dir().join(format!("tpt-app-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let manifest = dir.join("tpt-shell.toml");
        std::fs::write(
            &manifest,
            "[app]\nname = \"demo\"\n[audit]\nstore_params = true\nredact = [\"password\"]\n",
        )
        .expect("write manifest");
        let app = App::new()
            .manifest_audit(&manifest, dir.join("audit.jsonl"))
            .expect("manifest read");
        assert_eq!(
            app.audit,
            Some((
                dir.join("audit.jsonl"),
                Policy {
                    store_params: true,
                    redact_keys: vec!["password".into()],
                }
            ))
        );
        let no_section = dir.join("plain.toml");
        std::fs::write(&no_section, "[app]\nname = \"demo\"\n").expect("write");
        let plain = App::new()
            .manifest_audit(&no_section, "x.jsonl")
            .expect("read");
        assert_eq!(plain.audit.map(|(_, p)| p), Some(Policy::default()));
        assert!(App::new()
            .manifest_audit(&dir.join("missing.toml"), "x")
            .is_err());
        let _ = std::fs::remove_dir_all(&dir);
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
    fn the_contract_is_written_only_when_the_app_has_one() {
        let dir = std::env::temp_dir().join(format!("tpt-app-contract-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("contract.json");
        assert!(App::new().write_contract(&path).is_err());
        assert!(!path.exists());
        App::new()
            .contract(tpt_shell_ipc::contract::Contract::new())
            .write_contract(&path)
            .expect("write");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("contract_version"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn the_loader_is_looked_for_beside_the_program() {
        // The test binary has no WebView2Loader.dll beside it, so nothing is found.
        assert_eq!(loader_beside_program(), None);
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
