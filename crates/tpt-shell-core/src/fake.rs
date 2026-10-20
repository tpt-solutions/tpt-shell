// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! In-memory backend for unit tests (spec §5.2). No OS calls. Every call is recorded in `log`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::backend::{EventLoopBackend, EventLoopProxy, WebviewBackend, WindowBackend, WindowSpec};
use crate::error::{Error, Result};
use crate::geometry::LogicalSize;

/// Window handle used by the fake backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct FakeWindow {
    pub spec: WindowSpec,
    pub visible: bool,
    pub fullscreen: bool,
    pub html: Option<String>,
    pub url: Option<String>,
    pub messages: Vec<String>,
    pub closed: bool,
}

type Task = Box<dyn FnOnce() + Send>;

/// Proxy that queues tasks. `FakeBackend::drain` runs them.
#[derive(Clone, Default)]
pub struct FakeProxy {
    queue: Arc<Mutex<VecDeque<Task>>>,
}

impl EventLoopProxy for FakeProxy {
    fn post(&self, task: Task) -> Result<()> {
        self.queue
            .lock()
            .map_err(|_| Error::Backend("proxy queue poisoned".into()))?
            .push_back(task);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeBackend {
    windows: Vec<FakeWindow>,
    proxy: FakeProxy,
    pub quit_requested: bool,
    pub log: Vec<String>,
}

impl FakeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn window(&self, id: FakeId) -> Result<&FakeWindow> {
        self.windows
            .get(id.0 as usize)
            .ok_or(Error::UnknownWindow(id.0))
    }

    fn window_mut(&mut self, id: FakeId) -> Result<&mut FakeWindow> {
        self.windows
            .get_mut(id.0 as usize)
            .ok_or(Error::UnknownWindow(id.0))
    }

    /// Runs and clears every task posted through a proxy. Returns how many ran.
    pub fn drain(&self) -> usize {
        let tasks: Vec<Task> = match self.proxy.queue.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => return 0,
        };
        let n = tasks.len();
        for t in tasks {
            t();
        }
        n
    }
}

impl EventLoopBackend for FakeBackend {
    type Proxy = FakeProxy;

    fn proxy(&self) -> FakeProxy {
        self.proxy.clone()
    }

    fn quit(&mut self) {
        self.quit_requested = true;
        self.log.push("quit".into());
    }
}

impl WindowBackend for FakeBackend {
    type Id = FakeId;

    fn create(&mut self, spec: &WindowSpec) -> Result<FakeId> {
        self.log.push(format!("create {}", spec.name));
        self.windows.push(FakeWindow {
            spec: spec.clone(),
            visible: false,
            fullscreen: false,
            html: None,
            url: None,
            messages: Vec::new(),
            closed: false,
        });
        Ok(FakeId(self.windows.len() as u64 - 1))
    }

    fn show(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.visible = true;
        self.log.push(format!("show {}", id.0));
        Ok(())
    }

    fn hide(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.visible = false;
        self.log.push(format!("hide {}", id.0));
        Ok(())
    }

    fn set_title(&mut self, id: FakeId, title: &str) -> Result<()> {
        self.window_mut(id)?.spec.title = title.to_string();
        Ok(())
    }

    fn set_size(&mut self, id: FakeId, size: LogicalSize) -> Result<()> {
        self.window_mut(id)?.spec.size = size;
        Ok(())
    }

    fn set_fullscreen(&mut self, id: FakeId, on: bool) -> Result<()> {
        self.window_mut(id)?.fullscreen = on;
        Ok(())
    }

    fn close(&mut self, id: FakeId) -> Result<()> {
        self.window_mut(id)?.closed = true;
        self.log.push(format!("close {}", id.0));
        Ok(())
    }
}

impl WebviewBackend for FakeBackend {
    fn load_html(&mut self, id: FakeId, html: &str) -> Result<()> {
        let w = self.window_mut(id)?;
        w.html = Some(html.to_string());
        w.url = None;
        Ok(())
    }

    fn navigate(&mut self, id: FakeId, url: &str) -> Result<()> {
        if !url.starts_with("app://") {
            return Err(Error::Backend(format!("navigation blocked: {url}")));
        }
        let w = self.window_mut(id)?;
        w.url = Some(url.to_string());
        w.html = None;
        Ok(())
    }

    fn eval(&mut self, id: FakeId, script: &str) -> Result<()> {
        self.log.push(format!("eval {} {}", id.0, script));
        self.window(id).map(|_| ())
    }

    fn post_message(&mut self, id: FakeId, message: &str) -> Result<()> {
        self.window_mut(id)?.messages.push(message.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> WindowSpec {
        WindowSpec {
            name: name.into(),
            title: "t".into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
        }
    }

    #[test]
    fn create_show_and_load() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        b.show(id).unwrap();
        b.load_html(id, "<p>hi</p>").unwrap();
        let w = b.window(id).unwrap();
        assert!(w.visible);
        assert_eq!(w.html.as_deref(), Some("<p>hi</p>"));
    }

    #[test]
    fn navigation_only_allows_app_scheme() {
        let mut b = FakeBackend::new();
        let id = b.create(&spec("main")).unwrap();
        assert!(b.navigate(id, "app://localhost/index.html").is_ok());
        assert!(b.navigate(id, "https://example.com").is_err());
        assert!(b.navigate(id, "file:///C:/secret.txt").is_err());
        assert_eq!(
            b.window(id).unwrap().url.as_deref(),
            Some("app://localhost/index.html")
        );
    }

    #[test]
    fn unknown_window_is_an_error() {
        let mut b = FakeBackend::new();
        assert_eq!(b.show(FakeId(7)), Err(Error::UnknownWindow(7)));
    }

    #[test]
    fn proxy_posts_run_on_drain() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let b = FakeBackend::new();
        let proxy = b.proxy();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            proxy
                .post(Box::new(move || {
                    h.fetch_add(1, Ordering::SeqCst);
                }))
                .unwrap();
        })
        .join()
        .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert_eq!(b.drain(), 1);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn quit_is_recorded() {
        let mut b = FakeBackend::new();
        b.quit();
        assert!(b.quit_requested);
    }
}
