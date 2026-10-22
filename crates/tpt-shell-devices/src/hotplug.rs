// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Device discovery and hotplug events (spec §10.2, M7), opt-in.
//!
//! The watcher is given a list of present devices, which the caller obtained from a listing
//! that its scope filters, and reports what was added or removed since the last call. Nothing
//! is watched until the app opts in with [`Watch::opted_in`]; the default watcher reports
//! nothing and never sees a list, so it does not notice devices at all.
//!
//! Support: Partial. Events come from polling, not from OS notifications: [`Watch::poll_native`]
//! lists devices through [`crate::native::discover`] (macOS and Linux serial, hidraw, usbfs and
//! IOKit HID; Windows has none) and reports the difference. A caller can still supply the list.

use crate::id::DeviceId;

/// One change in the set of present devices.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(DeviceId),
    Removed(DeviceId),
}

/// Remembers the devices seen at the last poll and reports the difference.
///
/// Support: Partial. Polling only; see the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watch {
    enabled: bool,
    known: Vec<DeviceId>,
}

impl Watch {
    /// A watcher that reports nothing. The default.
    ///
    /// Support: Full.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// A watcher that reports changes. Use it only when the app asks for hotplug events.
    ///
    /// Support: Full.
    pub fn opted_in() -> Self {
        Watch {
            enabled: true,
            known: Vec::new(),
        }
    }

    /// Support: Full.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Compares `present` with the last poll. A disabled watcher returns nothing and keeps no
    /// state. The first poll of an enabled watcher reports every device as added.
    ///
    /// Support: Full.
    pub fn update(&mut self, present: &[DeviceId]) -> Vec<Change> {
        if !self.enabled {
            return Vec::new();
        }
        let mut changes = Vec::new();
        for id in present {
            if !self.known.contains(id) {
                changes.push(Change::Added(id.clone()));
            }
        }
        for id in &self.known {
            if !present.contains(id) {
                changes.push(Change::Removed(id.clone()));
            }
        }
        self.known = present.to_vec();
        changes
    }

    /// Lists the devices `scope` covers through the platform and compares them with the last
    /// poll. A disabled watcher does not list anything, so it never touches the system.
    ///
    /// Support: Partial. Polling, and no listing on Windows (the result is then always empty).
    pub fn poll_native(&mut self, scope: &crate::id::DeviceScope) -> Vec<Change> {
        if !self.enabled {
            return Vec::new();
        }
        let present = crate::native::discover(scope);
        self.update(&present)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> DeviceId {
        text.parse().expect("id")
    }

    #[test]
    fn a_disabled_watcher_reports_nothing_and_keeps_nothing() {
        let mut watch = Watch::disabled();
        assert!(!watch.is_enabled());
        assert!(watch.update(&[id("serial:COM1")]).is_empty());
        assert_eq!(watch, Watch::disabled());
    }

    #[test]
    fn a_disabled_watcher_does_not_list_the_system() {
        let mut watch = Watch::disabled();
        let scope = crate::id::DeviceScope::Serial { port: "*".into() };
        assert!(watch.poll_native(&scope).is_empty());
    }

    #[test]
    fn an_opted_in_watcher_polls_the_platform_without_failing() {
        let mut watch = Watch::opted_in();
        let scope = crate::id::DeviceScope::Hid {
            vendor: Some(0xffff),
            product: Some(0xfffe),
        };
        assert!(watch.poll_native(&scope).is_empty());
        assert!(watch.poll_native(&scope).is_empty());
    }

    #[test]
    fn an_opted_in_watcher_reports_adds_and_removes() {
        let mut watch = Watch::opted_in();
        let a = id("serial:COM1");
        let b = id("hid:1a2b:0001");
        assert_eq!(
            watch.update(std::slice::from_ref(&a)),
            vec![Change::Added(a.clone())]
        );
        assert!(watch.update(std::slice::from_ref(&a)).is_empty());
        assert_eq!(
            watch.update(std::slice::from_ref(&b)),
            vec![Change::Added(b.clone()), Change::Removed(a.clone())]
        );
    }
}
