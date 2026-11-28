// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The tray icon on Linux: a StatusNotifier item (see `dbus::tray`) whose left clicks reach the
//! app's callback on the UI thread.
//!
//! The item's thread only posts the click to the UI thread through the event loop's proxy. The
//! callback and the running item stay on the UI thread that set them, so the callback does not
//! need to be `Send`. The item has no menu, so a call with menu items is refused.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use tpt_shell_core::backend::EventLoopProxy;
use tpt_shell_core::platform::{TrayItem, TRAY_ACTIVATE};
use tpt_shell_core::{Error, Result};

use crate::dbus::tray::TrayOptions;

/// The icon a Linux tray shows when the app does not choose one. The host draws it.
const ICON: &str = "application-x-executable";

type Callback = Rc<RefCell<Box<dyn FnMut(u32)>>>;

/// What the UI thread keeps for the shown icon: the callback, and the running item, which stops
/// when this is dropped.
struct Slot {
    on_select: Callback,
    _item: Box<dyn Any>,
}

thread_local! {
    static SLOT: RefCell<Option<Slot>> = const { RefCell::new(None) };
}

/// Shows the tray icon, replacing one shown before. `start` creates the item from its options and
/// its click handler; the handler is called on the item's thread and posts each click to the UI
/// thread through `proxy`.
pub fn set<P, H>(
    proxy: P,
    tooltip: &str,
    items: &[TrayItem],
    on_select: Box<dyn FnMut(u32)>,
    start: impl FnOnce(TrayOptions, Box<dyn FnMut(i32, i32) + Send>) -> Result<H>,
) -> Result<()>
where
    P: EventLoopProxy,
    H: 'static,
{
    if !items.is_empty() {
        return Err(Error::Unsupported {
            feature: "tray menus",
        });
    }
    let options = TrayOptions {
        app_id: app_id(),
        title: tooltip.to_string(),
        icon_name: ICON.to_string(),
    };
    let on_activate: Box<dyn FnMut(i32, i32) + Send> = Box::new(move |_x, _y| {
        // A failed post means the loop has stopped, so there is nothing to tell.
        let _ = proxy.post(Box::new(|| deliver(TRAY_ACTIVATE)));
    });
    remove();
    let item = start(options, on_activate)?;
    let slot = Slot {
        on_select: Rc::new(RefCell::new(on_select)),
        _item: Box::new(item),
    };
    SLOT.with(|shown| *shown.borrow_mut() = Some(slot));
    Ok(())
}

/// Removes the icon. Nothing happens when none is shown.
pub fn remove() {
    let old = SLOT.with(|shown| shown.borrow_mut().take());
    drop(old);
}

/// Runs the callback with `id` on the UI thread. A click after the icon was removed does nothing.
fn deliver(id: u32) {
    let callback = SLOT.with(|shown| shown.borrow().as_ref().map(|s| Rc::clone(&s.on_select)));
    if let Some(callback) = callback {
        (callback.borrow_mut())(id);
    }
}

/// The app id the item is registered under: the program's file name, with characters the host
/// does not accept replaced by `-`.
fn app_id() -> String {
    crate::desktop::app_id_for(&std::env::current_exe().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    type Task = Box<dyn FnOnce() + Send>;

    /// A proxy that queues tasks; the test runs them on its own thread, which is the "UI" thread.
    #[derive(Clone, Default)]
    struct QueueProxy(Arc<Mutex<VecDeque<Task>>>);

    impl EventLoopProxy for QueueProxy {
        fn post(&self, task: Task) -> Result<()> {
            self.0.lock().unwrap().push_back(task);
            Ok(())
        }
    }

    impl QueueProxy {
        fn run_queued(&self) {
            loop {
                let next = self.0.lock().unwrap().pop_front();
                match next {
                    Some(task) => task(),
                    None => break,
                }
            }
        }
    }

    type Handler = Box<dyn FnMut(i32, i32) + Send>;
    type Click = Arc<Mutex<Option<Handler>>>;

    /// A starter that keeps the click handler, so the test can click.
    fn starter(clicks: Click) -> impl FnOnce(TrayOptions, Handler) -> Result<()> {
        move |_options, on_activate| {
            *clicks.lock().unwrap() = Some(on_activate);
            Ok(())
        }
    }

    fn click(clicks: &Click) {
        let mut guard = clicks.lock().unwrap();
        (guard.as_mut().expect("the item is running"))(0, 0);
    }

    #[test]
    fn a_click_reaches_the_callback_on_the_ui_thread() {
        let proxy = QueueProxy::default();
        let clicks: Click = Arc::default();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        set(
            proxy.clone(),
            "Demo",
            &[],
            Box::new(move |id| sink.borrow_mut().push(id)),
            starter(Arc::clone(&clicks)),
        )
        .expect("set");
        click(&clicks);
        assert!(
            seen.borrow().is_empty(),
            "the click waits for the UI thread"
        );
        proxy.run_queued();
        assert_eq!(*seen.borrow(), vec![TRAY_ACTIVATE]);
        remove();
    }

    #[test]
    fn menu_items_are_refused_and_nothing_is_shown() {
        let proxy = QueueProxy::default();
        let clicks: Click = Arc::default();
        let items = [TrayItem {
            id: 3,
            label: "Quit".into(),
        }];
        let result = set(
            proxy,
            "Demo",
            &items,
            Box::new(|_| {}),
            starter(Arc::clone(&clicks)),
        );
        assert!(matches!(result, Err(Error::Unsupported { .. })));
        assert!(clicks.lock().unwrap().is_none());
    }

    #[test]
    fn a_click_after_removal_does_nothing() {
        let proxy = QueueProxy::default();
        let clicks: Click = Arc::default();
        let seen = Rc::new(RefCell::new(0));
        let sink = Rc::clone(&seen);
        set(
            proxy.clone(),
            "Demo",
            &[],
            Box::new(move |_| *sink.borrow_mut() += 1),
            starter(Arc::clone(&clicks)),
        )
        .expect("set");
        click(&clicks);
        remove();
        proxy.run_queued();
        assert_eq!(*seen.borrow(), 0);
    }

    #[test]
    fn a_failed_start_leaves_no_icon() {
        let proxy = QueueProxy::default();
        let result = set(
            proxy,
            "Demo",
            &[],
            Box::new(|_| {}),
            |_options, _on_activate| -> Result<()> { Err(Error::Backend("no host".into())) },
        );
        assert!(result.is_err());
        SLOT.with(|shown| assert!(shown.borrow().is_none()));
    }

    #[test]
    fn the_app_id_uses_only_characters_the_host_accepts() {
        let id = app_id();
        assert!(!id.is_empty());
        assert!(id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')));
    }
}
