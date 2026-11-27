// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The app's side of the test driver (spec §10.6), built with the `test-driver` feature. The
//! driver writes its requests into a run directory; the app answers them on the UI thread from the
//! window: the page's snapshot and actions run as page scripts, and the screenshot is captured
//! with WebView2. The run directory is named by the `TPT_TEST_RUN` environment variable. It is
//! `run-<token>`, and the token is the name the channel checks every message against.
//!
//! Element handles are snapshot references. The page refuses a stale one, and the driver receives
//! that refusal as an error.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use tpt_shell_agent::page::{self, Action};
use tpt_shell_platform_windows::backend::{capture_window_png, WindowId};
use tpt_shell_test::channel::AppChannel;
use tpt_shell_test::runner::{self, Host, Node};

use crate::agent_snapshot::PageScript;

/// How long a page script or a capture may take, on the UI thread.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The environment variable that names the run directory.
pub(crate) const RUN_ENV: &str = "TPT_TEST_RUN";

/// The window the driver acts on, filled in when the window exists, and the page scripts.
pub(crate) struct WindowHost {
    window: Rc<Cell<Option<WindowId>>>,
    page: PageScript,
}

impl WindowHost {
    pub(crate) fn new(window: Rc<Cell<Option<WindowId>>>, page: PageScript) -> Self {
        WindowHost { window, page }
    }

    /// Runs a page script and parses its JSON answer.
    fn page_value(&mut self, script: &str) -> Result<Value, String> {
        let text = (self.page)(script)?.ok_or("the page gave no value")?;
        serde_json::from_str(&text).map_err(|e| format!("the page answer is not JSON: {e}"))
    }

    /// Runs an action on the page and returns its data.
    fn act(&mut self, action: &Action) -> Result<Value, String> {
        let text = (self.page)(&page::action_script(action))?
            .ok_or("the page gave no result for the action")?;
        page::parse_action_result(&text)
    }
}

impl Host for WindowHost {
    fn nodes(&mut self) -> Result<Vec<Node>, String> {
        let text = (self.page)(&page::snapshot_script())?.ok_or("the page gave no snapshot")?;
        // A page that has not loaded its scripts yet answers null. That is no elements yet, so a
        // find retries instead of failing.
        if text.trim() == "null" {
            return Ok(Vec::new());
        }
        let snapshot = page::parse_snapshot(&text)?;
        Ok(snapshot
            .tree
            .into_iter()
            .map(|n| Node {
                reference: n.ref_id,
                role: n.role,
                name: if n.name.is_empty() {
                    None
                } else {
                    Some(n.name)
                },
            })
            .collect())
    }

    fn click(&mut self, reference: u32) -> Result<(), String> {
        self.act(&Action::Click { ref_id: reference }).map(|_| ())
    }

    fn type_text(&mut self, reference: u32, text: &str) -> Result<(), String> {
        self.act(&Action::Type {
            ref_id: reference,
            text: text.to_string(),
        })
        .map(|_| ())
    }

    fn press(&mut self, key: &str) -> Result<(), String> {
        self.act(&Action::Key {
            ref_id: None,
            key: key.to_string(),
        })
        .map(|_| ())
    }

    fn capture(&mut self) -> Result<Vec<u8>, String> {
        let id = self.window.get().ok_or("no window is open yet")?;
        capture_window_png(id, TIMEOUT).map_err(|e| e.to_string())
    }

    fn dom(&mut self) -> Result<Value, String> {
        let html = self.page_value("document.documentElement.outerHTML")?;
        Ok(json!({ "html": html }))
    }

    fn eval(&mut self, script: &str) -> Result<Value, String> {
        // The script is an expression, so its value is the answer.
        self.page_value(&format!("({script})"))
    }
}

/// Opens the run directory named by [`RUN_ENV`], or returns `None` when the variable is not set
/// (an ordinary run, not a test run).
pub(crate) fn open_channel() -> crate::Result<Option<AppChannel>> {
    let Some(dir) = std::env::var_os(RUN_ENV) else {
        return Ok(None);
    };
    let dir = Path::new(&dir);
    let token = dir
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("run-"))
        .ok_or_else(|| {
            crate::Error::Backend(format!(
                "{RUN_ENV} must name a run directory called run-<token>, not {}",
                dir.display()
            ))
        })?;
    AppChannel::open(dir, token)
        .map(Some)
        .map_err(|e| crate::Error::Backend(format!("{}: {}", e.code, e.message)))
}

/// Answers the driver's pending requests, a few at a time so one busy tick cannot stall the UI
/// thread. Called on the UI thread from a timer.
pub(crate) fn serve_pending(channel: &mut AppChannel, host: &mut WindowHost) {
    for _ in 0..8 {
        if !runner::serve(channel, host) {
            break;
        }
    }
}
