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

use tpt_shell_capabilities::Grant;
use tpt_shell_core::backend::Asset;
#[cfg(windows)]
use tpt_shell_core::backend::WindowSpec;
#[cfg(windows)]
use tpt_shell_core::geometry::LogicalSize;
use tpt_shell_core::Error;
use tpt_shell_core::Result;
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
}

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

    /// Starts the app and runs it until its last window closes or the app quits.
    pub fn run(self) -> Result<()> {
        #[cfg(windows)]
        {
            self.build()?.run()
        }
        #[cfg(not(windows))]
        {
            Err(Error::Unsupported {
                feature: "running an app on this platform",
            })
        }
    }

    /// Creates the windows and wires them, without running the event loop. Windows only.
    #[cfg(windows)]
    pub fn build(self) -> Result<Running> {
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
            backend.set_message_handler(
                id,
                Box::new(move |text| {
                    let mut prompt = NativeConsent::for_window(id);
                    Some(router.handle_authorized(
                        &caller,
                        &mut engine.borrow_mut(),
                        &mut prompt,
                        text.as_bytes(),
                        &Limits::default(),
                    ))
                }),
            )?;
            let assets = assets.clone();
            backend.set_asset_source(id, Box::new(move |path| assets.get(path).cloned()))?;
            backend.navigate(id, &format!("{APP_ORIGIN}/{}", window.page))?;
            windows.push((window.name, id));
        }
        Ok(Running { backend, windows })
    }
}

/// A started app: its windows exist and their pages are loaded. Windows only.
#[cfg(windows)]
pub struct Running {
    backend: tpt_shell_platform_windows::backend::WindowsBackend,
    windows: Vec<(String, tpt_shell_platform_windows::backend::WindowId)>,
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
