// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The app builder (spec §7.1). It gathers windows, grants and commands, then starts them.
//!
//! What runs today: Windows, with WebView2. Every window loads the HTML the app gives it, and
//! its page messages go through the capability-checked router with the window's own grants.
//! Consent prompts are refused until the native consent dialog exists. On other platforms
//! [`App::run`] and [`App::build`] return [`Error::Unsupported`].

use std::path::PathBuf;

use tpt_shell_capabilities::Grant;
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
    html: Option<String>,
}

impl Window {
    /// A window called `name`. The name is the window's identity in grants.
    pub fn new(name: &str) -> Self {
        Window {
            name: name.to_string(),
            title: name.to_string(),
            width: 800.0,
            height: 600.0,
            html: None,
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

    /// The document the window shows. The app supplies it, so it counts as app content.
    pub fn html(mut self, html: &str) -> Self {
        self.html = Some(html.to_string());
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

    #[cfg(windows)]
    fn document(&self) -> String {
        self.html
            .clone()
            .unwrap_or_else(|| format!("<!doctype html><title>{}</title>", self.title))
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
                    let mut prompt = RefuseConsent;
                    Some(router.handle_authorized(
                        &caller,
                        &mut engine.borrow_mut(),
                        &mut prompt,
                        text.as_bytes(),
                        &Limits::default(),
                    ))
                }),
            )?;
            backend.load_html(id, &window.document())?;
            windows.push((window.name, id));
        }
        Ok(Running { backend, windows })
    }
}

/// Refuses every consent prompt. A consent grant is refused until the native consent dialog
/// exists on each platform.
#[cfg(windows)]
struct RefuseConsent;

#[cfg(windows)]
impl tpt_shell_capabilities::ConsentPrompt for RefuseConsent {
    fn ask(&mut self, _window: &str, _capability: &str) -> bool {
        false
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
    fn a_window_keeps_its_name_title_size_and_document() {
        let w = Window::new("main")
            .title("Demo")
            .size(1100.0, 700.0)
            .html("<p>x</p>");
        let spec = w.spec();
        assert_eq!(spec.name, "main");
        assert_eq!(spec.title, "Demo");
        assert_eq!((spec.size.width, spec.size.height), (1100.0, 700.0));
        assert!(spec.allow_navigation.is_empty());
        assert_eq!(w.document(), "<p>x</p>");
    }

    #[cfg(windows)]
    #[test]
    fn a_window_without_html_gets_a_titled_placeholder() {
        let w = Window::new("main").title("Demo");
        assert_eq!(w.document(), "<!doctype html><title>Demo</title>");
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
