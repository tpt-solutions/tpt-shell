// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Device discovery and hotplug events (spec §10.2, M7), opt-in.
//!
//! The watcher is given a list of present devices, which the caller obtained from a listing
//! that its scope filters, and reports what was added or removed since the last call. Nothing
//! is watched until the app opts in with [`Watch::opted_in`]; the default watcher reports
//! nothing and never sees a list, so it does not notice devices at all.
//!
//! Support: Partial. The diff is complete, but no platform listing feeds it yet (the Win32, IOKit,
//! hidraw and usbfs enumerations are not built), so today a caller supplies the list.

use crate::id::DeviceId;

/// One change in the set of present devices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Added(DeviceId),
    Removed(DeviceId),
}

/// Remembers the devices seen at the last poll and reports the difference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watch {
    enabled: bool,
    known: Vec<DeviceId>,
}

impl Watch {
    /// A watcher that reports nothing. The default.
    pub fn disabled() -> Self {
        Self::default()
    }

    /// A watcher that reports changes. Use it only when the app asks for hotplug events.
    pub fn opted_in() -> Self {
        Watch {
            enabled: true,
            known: Vec::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Compares `present` with the last poll. A disabled watcher returns nothing and keeps no
    /// state. The first poll of an enabled watcher reports every device as added.
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
