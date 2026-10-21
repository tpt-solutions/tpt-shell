// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Backend trait set (spec §5.2). Platform crates implement these.
//!
//! This is the M1 subset: enough for a window with a webview to be created, shown,
//! loaded and messaged. Menus, tray, dialogs, clipboard and the rest of the
//! `Platform` trait are later milestones.

use crate::error::{Error, Result};
use crate::geometry::{LogicalRect, LogicalSize};

/// Settings used to create a window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub name: String,
    pub title: String,
    pub size: LogicalSize,
    /// `https://` URL prefixes this window may navigate to. Empty: app content only.
    pub allow_navigation: Vec<String>,
}

/// Owns the UI thread's event loop.
pub trait EventLoopBackend {
    /// Handle for posting work to the UI thread from any thread.
    type Proxy: EventLoopProxy;

    fn proxy(&self) -> Self::Proxy;

    /// Asks the loop to stop. Safe to call from the UI thread.
    fn quit(&mut self);
}

/// Cross-thread handle to the UI thread. The only way to touch UI from another thread.
pub trait EventLoopProxy: Send + Sync + Clone + 'static {
    /// Queues `task` to run on the UI thread.
    fn post(&self, task: Box<dyn FnOnce() + Send>) -> Result<()>;
}

/// What happens when the user asks a window to close (title bar button, Alt+F4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloseAction {
    /// Close the window.
    #[default]
    Close,
    /// Keep the window; hide it instead. The app can show it again later.
    Hide,
}

/// System backdrop material behind a window's client area (spec §10.x, Windows 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Backdrop {
    #[default]
    None,
    Mica,
    Acrylic,
    Tabbed,
}

/// Window management.
///
/// Methods past `close` are optional. Their default is [`Error::Unsupported`], so a backend
/// that does not provide one says so instead of silently ignoring the request.
pub trait WindowBackend {
    /// Backend-specific window handle.
    type Id: Copy + Eq + std::fmt::Debug;

    fn create(&mut self, spec: &WindowSpec) -> Result<Self::Id>;
    fn show(&mut self, id: Self::Id) -> Result<()>;
    fn hide(&mut self, id: Self::Id) -> Result<()>;
    fn set_title(&mut self, id: Self::Id, title: &str) -> Result<()>;
    fn set_size(&mut self, id: Self::Id, size: LogicalSize) -> Result<()>;
    fn set_fullscreen(&mut self, id: Self::Id, on: bool) -> Result<()>;
    fn close(&mut self, id: Self::Id) -> Result<()>;

    /// Removes the title bar and border.
    fn set_frameless(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "frameless",
        })
    }

    fn set_always_on_top(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "always-on-top",
        })
    }

    /// Makes the window background see-through where the page does not paint.
    fn set_transparent(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "transparency",
        })
    }

    fn set_backdrop(&mut self, _id: Self::Id, _backdrop: Backdrop) -> Result<()> {
        Err(Error::Unsupported {
            feature: "backdrop",
        })
    }

    /// Regions (in logical units, window coordinates) that drag the window when pressed.
    /// An empty slice removes all drag regions.
    fn set_drag_regions(&mut self, _id: Self::Id, _regions: &[LogicalRect]) -> Result<()> {
        Err(Error::Unsupported {
            feature: "drag regions",
        })
    }

    /// Creates a window owned by `parent`. It closes with its parent and stays above it.
    fn create_child(&mut self, _parent: Self::Id, _spec: &WindowSpec) -> Result<Self::Id> {
        Err(Error::Unsupported {
            feature: "child windows",
        })
    }

    /// Makes `child` modal to its owner: while on, the owner ignores input. Turning it off, or
    /// closing the child, gives the owner its input back.
    fn set_modal(&mut self, _child: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "modal windows",
        })
    }

    /// Chooses what a user close request does. Applies to later requests only.
    fn set_close_action(&mut self, _id: Self::Id, _action: CloseAction) -> Result<()> {
        Err(Error::Unsupported {
            feature: "close handling",
        })
    }

    /// Draws the title bar in the dark style (Windows 10 1809 and later). Off restores the light
    /// style. Only the frame changes; the page's own colours are not touched.
    fn set_dark_title_bar(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "dark title bar",
        })
    }
}

/// Whether developer tools may be switched on or opened for a window (spec §9). Release builds
/// keep them off: turning them on there is refused. Debug builds may switch them either way.
pub fn devtools_policy(release_build: bool, on: bool) -> Result<()> {
    if release_build && on {
        return Err(Error::Unsupported {
            feature: "developer tools in release builds",
        });
    }
    Ok(())
}

/// Colour scheme a page sees through `prefers-color-scheme` (spec 10.x, theme sync).
/// `System` follows the operating system and changes with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PreferredScheme {
    #[default]
    System,
    Light,
    Dark,
}

/// Receives a message the page posted, and returns the reply to post back, if there is one.
pub type MessageHandler = Box<dyn FnMut(&str) -> Option<String>>;

/// A file the app serves from its own origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Finds the asset for a path under the app origin, such as `index.html` or `app.js`. `None`
/// is answered as not found.
pub type AssetSource = Box<dyn Fn(&str) -> Option<Asset>>;

/// Webview inside a window.
pub trait WebviewBackend: WindowBackend {
    /// Loads an HTML document into the window's webview.
    fn load_html(&mut self, id: Self::Id, html: &str) -> Result<()>;
    /// Navigates to an `app://` URL. Other schemes are rejected by the navigation policy (spec §9).
    fn navigate(&mut self, id: Self::Id, url: &str) -> Result<()>;
    /// Runs a script in the page. Internal use only; not exposed to agents (spec §10.4).
    fn eval(&mut self, id: Self::Id, script: &str) -> Result<()>;
    /// Sends a message string to the page's `message` listener.
    fn post_message(&mut self, id: Self::Id, message: &str) -> Result<()>;
    /// Sets the handler for messages the page posts to the shell. A reply the handler returns is
    /// posted back to the page. Replaces any earlier handler. Backends that cannot receive page
    /// messages yet return [`Error::Unsupported`].
    fn set_message_handler(&mut self, id: Self::Id, handler: MessageHandler) -> Result<()> {
        let _ = (id, handler);
        Err(Error::Unsupported {
            feature: "page messages",
        })
    }
    /// Sets where the window's app pages come from. Requests for the app origin are answered
    /// from `source`; a path it does not return is answered as not found. Backends without it
    /// return [`Error::Unsupported`].
    fn set_asset_source(&mut self, id: Self::Id, source: AssetSource) -> Result<()> {
        let _ = (id, source);
        Err(Error::Unsupported {
            feature: "app assets",
        })
    }
    /// Captures what the window's page shows, as PNG bytes.
    fn snapshot(&mut self, _id: Self::Id) -> Result<Vec<u8>> {
        Err(Error::Unsupported {
            feature: "snapshot",
        })
    }
    /// Turns the developer tools on or off for the window's page. Release builds refuse `on`
    /// (see [`devtools_policy`]).
    fn set_devtools(&mut self, _id: Self::Id, _on: bool) -> Result<()> {
        Err(Error::Unsupported {
            feature: "devtools toggle",
        })
    }
    /// Opens the developer tools in their own window. Refused unless they are enabled.
    fn open_devtools(&mut self, _id: Self::Id) -> Result<()> {
        Err(Error::Unsupported {
            feature: "devtools toggle",
        })
    }
    /// Sets the colour scheme the page's `prefers-color-scheme` media query sees.
    fn set_preferred_color_scheme(
        &mut self,
        _id: Self::Id,
        _scheme: PreferredScheme,
    ) -> Result<()> {
        Err(Error::Unsupported {
            feature: "preferred colour scheme",
        })
    }
}
