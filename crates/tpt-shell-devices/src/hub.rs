// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The device hub (spec §10.2): the devices the shell can see, and the only way to open one.
//!
//! Opening checks the caller's scope first, so a grant for `serial:COM*` cannot open a printer.
//! Each open, write and close is reported to the audit sink with the number of bytes, never the
//! bytes themselves (payload logging is off by default). A device is opened by one caller at a
//! time, and it is not offered again until the hub is rebuilt.

use std::sync::{Arc, Mutex};

use crate::id::{DeviceId, DeviceScope};
use crate::transport::{DeviceError, Transport};

/// What happened to a device, for the audit log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    pub device: DeviceId,
    /// Bytes written, for writes. Zero for opens and closes.
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Open,
    Write,
    Close,
}

type Sink = Arc<Mutex<dyn FnMut(&Event) + Send>>;

/// The devices that are present, and the audit sink for their use.
pub struct DeviceHub {
    available: Vec<Box<dyn Transport>>,
    sink: Sink,
}

impl DeviceHub {
    /// A hub over `devices`. `audit` is called for every open, write and close.
    pub fn new(
        devices: Vec<Box<dyn Transport>>,
        audit: impl FnMut(&Event) + Send + 'static,
    ) -> Self {
        DeviceHub {
            available: devices,
            sink: Arc::new(Mutex::new(audit)),
        }
    }

    /// The identifiers of the present devices that `scope` covers.
    pub fn list(&self, scope: &DeviceScope) -> Vec<DeviceId> {
        self.available
            .iter()
            .map(|d| d.id().clone())
            .filter(|id| scope.matches(id))
            .collect()
    }

    /// Opens the device `id`, if the caller's `scope` covers it and it is present and free.
    pub fn open(&mut self, id: &DeviceId, scope: &DeviceScope) -> Result<OpenDevice, DeviceError> {
        if !scope.matches(id) {
            return Err(DeviceError::Denied(format!(
                "{id} is outside the granted scope"
            )));
        }
        let index = self
            .available
            .iter()
            .position(|d| d.id() == id)
            .ok_or_else(|| DeviceError::Denied(format!("{id} is not present or already open")))?;
        let transport = self.available.remove(index);
        record(&self.sink, EventKind::Open, id, 0);
        Ok(OpenDevice {
            transport,
            sink: self.sink.clone(),
        })
    }
}

/// An open device. Dropping it closes the device, and the close is audited.
pub struct OpenDevice {
    transport: Box<dyn Transport>,
    sink: Sink,
}

impl OpenDevice {
    pub fn id(&self) -> &DeviceId {
        self.transport.id()
    }

    /// Sends `bytes`, and audits the number of bytes that were taken.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        self.transport.write(bytes)?;
        record(
            &self.sink,
            EventKind::Write,
            self.transport.id(),
            bytes.len(),
        );
        Ok(())
    }

    /// Reads up to `max` bytes. Reads are not audited.
    pub fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        self.transport.read(max)
    }
}

impl Drop for OpenDevice {
    fn drop(&mut self) {
        record(&self.sink, EventKind::Close, self.transport.id(), 0);
    }
}

fn record(sink: &Sink, kind: EventKind, device: &DeviceId, bytes: usize) {
    let event = Event {
        kind,
        device: device.clone(),
        bytes,
    };
    // A poisoned sink still records: the audit must not be lost to an earlier panic.
    let mut sink = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    (sink)(&event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::virtual_devices::VirtualSerial;

    fn port(name: &str) -> DeviceId {
        DeviceId::Serial {
            port: name.to_string(),
        }
    }

    #[test]
    fn a_device_is_listed_opened_once_and_audited() {
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let log = events.clone();
        let mut hub = DeviceHub::new(
            vec![Box::new(VirtualSerial::loopback(port("COM7")))],
            move |e| log.lock().expect("log").push(e.clone()),
        );
        let scope = DeviceScope::Serial {
            port: "COM*".into(),
        };
        assert_eq!(hub.list(&scope), vec![port("COM7")]);

        {
            let mut open = hub.open(&port("COM7"), &scope).expect("open");
            open.write(b"abc").expect("write");
            assert!(hub.open(&port("COM7"), &scope).is_err(), "opened twice");
            assert_eq!(open.read(10).expect("read"), b"abc");
        }
        assert!(
            hub.list(&scope).is_empty(),
            "the device is not offered while open or after close"
        );

        let seen = events.lock().expect("events");
        let kinds: Vec<EventKind> = seen.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![EventKind::Open, EventKind::Write, EventKind::Close]
        );
        assert_eq!(seen[1].bytes, 3);
    }

    #[test]
    fn a_scope_that_does_not_cover_the_device_cannot_open_it() {
        let mut hub = DeviceHub::new(
            vec![Box::new(VirtualSerial::loopback(port("COM7")))],
            |_| {},
        );
        let narrow = DeviceScope::Serial {
            port: "COM1".into(),
        };
        assert!(matches!(
            hub.open(&port("COM7"), &narrow),
            Err(DeviceError::Denied(_))
        ));
        assert!(hub.list(&narrow).is_empty());
    }
}
