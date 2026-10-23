// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Per-window session hooks (Windows). The facade keeps the windows it opened and the URL it last
//! loaded in each. That lets the hooks act on a window by id: remove a closed window's blobs,
//! recover a page whose process failed (kiosk apps), and clear the session on an idle reset.
//!
//! All of this runs on the UI thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tpt_shell_core::Result;
use tpt_shell_kiosk::recovery::{CrashRecovery, Recovery};
use tpt_shell_platform_windows::backend::{clear_session, navigate_window, page_source, WindowId};
use tpt_shell_platform_windows::webview2::{ProcessFailure, APP_ORIGIN};

use crate::blob;

/// A kiosk window that fails more than `CRASH_LIMIT` times within `CRASH_WINDOW` goes home. These
/// are the facade's values, not a policy setting.
///
/// Support: Partial. Windows only.
pub const CRASH_WINDOW: Duration = Duration::from_secs(60);
/// Support: Partial. Windows only.
pub const CRASH_LIMIT: u32 = 3;

struct Pane {
    id: WindowId,
    name: String,
    /// The URL the shell last loaded in the window. Empty until the first navigation.
    url: String,
}

/// The windows the app opened, by id, with the URL the shell last loaded in each.
///
/// Support: Partial. Windows only.
#[derive(Default)]
pub struct Panes {
    list: RefCell<Vec<Pane>>,
}

impl Panes {
    /// Support: Partial. Windows only.
    pub fn add(&self, id: WindowId, name: &str) {
        self.list.borrow_mut().push(Pane {
            id,
            name: name.to_string(),
            url: String::new(),
        });
    }
    /// Support: Partial. Windows only.

    pub fn name_of(&self, id: WindowId) -> Option<String> {
        self.list
            .borrow()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
    }
    /// Support: Partial. Windows only.

    pub fn url_of(&self, id: WindowId) -> Option<String> {
        self.list
            .borrow()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.url.clone())
    }

    fn set_url(&self, id: WindowId, url: &str) {
        if let Some(pane) = self.list.borrow_mut().iter_mut().find(|p| p.id == id) {
            pane.url = url.to_string();
        }
    }
}

/// Loads `url` in window `id` and records it as the window's current URL.
///
/// Support: Partial. Windows only.
pub fn go(panes: &Panes, id: WindowId, url: &str) -> Result<()> {
    navigate_window(id, url)?;
    panes.set_url(id, url);
    Ok(())
}

/// The close hook: removes the blobs of a window when it closes. See `WindowsBackend::on_window_closed`.
///
/// Support: Partial. Windows only.
pub fn close_hook(panes: Rc<Panes>) -> impl FnMut(WindowId) {
    move |id| {
        if let Some(name) = panes.name_of(id) {
            blob::remove_window_blobs(&name);
        }
    }
}

/// The route a window's page is on now, for crash recovery to return to. A page that sends a
/// message is alive, so the document the view shows then is a good route, even when the page moved
/// itself without the shell's help. When the view cannot say, or shows something outside the app,
/// the last URL the shell loaded stands in.
///
/// Support: Partial. Windows only.
pub fn current_route(panes: &Panes, id: WindowId) -> Option<String> {
    match page_source(id).ok().as_deref().and_then(app_route) {
        Some(route) => Some(route.to_string()),
        None => panes.url_of(id),
    }
}

/// `source` when it is a page of the app, under the app origin. Anything else is not a route the
/// window should return to.
fn app_route(source: &str) -> Option<&str> {
    let under_origin =
        source == APP_ORIGIN || source.starts_with(&format!("{APP_ORIGIN}/"));
    under_origin.then_some(source)
}

/// The process-failed handler of a kiosk window: asks `recovery` what to load and loads it.
/// See `WindowsBackend::on_process_failed`.
///
/// Support: Partial. Windows only. Kiosk crash recovery.
pub fn recover(
    panes: Rc<Panes>,
    id: WindowId,
    recovery: Rc<RefCell<CrashRecovery>>,
    home: String,
) -> impl FnMut(ProcessFailure) {
    move |failure| {
        let decision = recovery.borrow_mut().page_failed(Instant::now());
        let url = match decision {
            Recovery::Reload(url) => url,
            Recovery::Home => home.clone(),
        };
        eprintln!("tpt-shell: a page process failed ({failure:?}); loading {url}");
        if let Err(e) = go(&panes, id, &url) {
            eprintln!("tpt-shell: cannot recover the page: {e}");
        }
    }
}

/// Clears the browsing data of the profile the windows share, then loads `home` in each window.
/// The clear is asynchronous: the windows go home when WebView2 reports it done, and also when
/// it fails (the failure is reported). If the clear cannot start, they go home at once.
///
/// Support: Partial. Windows only. Used by the kiosk idle reset.
pub fn clear_and_go_home(panes: Rc<Panes>, ids: Vec<WindowId>, home: String) {
    let Some(&first) = ids.first() else {
        return;
    };
    let go_home: Rc<dyn Fn()> = Rc::new(move || {
        for id in &ids {
            if let Err(e) = go(&panes, *id, &home) {
                eprintln!("tpt-shell: cannot return to the home page: {e}");
            }
        }
    });
    let done = go_home.clone();
    if let Err(e) = clear_session(first, move |hr| {
        if hr < 0 {
            eprintln!("tpt-shell: clearing the session failed (HRESULT {hr:#x})");
        }
        done();
    }) {
        eprintln!("tpt-shell: cannot clear the session: {e}");
        go_home();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_close_hook_removes_the_blobs_of_the_closed_window_only() {
        let panes = Rc::new(Panes::default());
        panes.add(WindowId(41), "session-test-closed");
        panes.add(WindowId(42), "session-test-open");
        let mine =
            blob::put_blob_for("session-test-closed", "text/plain", b"a".to_vec()).expect("put");
        let theirs =
            blob::put_blob_for("session-test-open", "text/plain", b"b".to_vec()).expect("put");
        let mut hook = close_hook(panes.clone());
        hook(WindowId(41));
        assert!(
            !blob::remove_blob(&mine),
            "the closed window's blob is gone"
        );
        assert!(blob::remove_blob(&theirs), "another window's blob stays");
        // An id the facade did not open is ignored.
        hook(WindowId(99));
    }

    #[test]
    fn the_panes_keep_the_last_url_the_shell_loaded() {
        let panes = Panes::default();
        panes.add(WindowId(7), "main");
        assert_eq!(panes.url_of(WindowId(7)).as_deref(), Some(""));
        panes.set_url(WindowId(7), "https://tpt-app.localhost/index.html");
        assert_eq!(
            panes.url_of(WindowId(7)).as_deref(),
            Some("https://tpt-app.localhost/index.html")
        );
        assert_eq!(panes.name_of(WindowId(7)).as_deref(), Some("main"));
        assert_eq!(panes.url_of(WindowId(8)), None);
    }

    #[test]
    fn only_pages_of_the_app_count_as_routes() {
        let page = format!("{APP_ORIGIN}/form?step=2");
        assert_eq!(app_route(&page), Some(page.as_str()));
        assert_eq!(app_route(APP_ORIGIN), Some(APP_ORIGIN));
        assert_eq!(app_route("https://tpt-app.localhost.example.com/form"), None);
        assert_eq!(app_route("https://example.com/"), None);
        assert_eq!(app_route(""), None);
    }
}
