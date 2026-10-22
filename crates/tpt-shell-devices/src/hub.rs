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
use crate::options::OpenOptions;
use crate::transport::{ControlRequest, DeviceError, Transport};

/// What happened to a device, for the audit log.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: EventKind,
    pub device: DeviceId,
    /// Bytes written, for writes. Zero for opens and closes.
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Support: Full.
pub enum EventKind {
    Open,
    Write,
    /// A control transfer. The byte count is the data stage sent or received.
    Control,
    Close,
}

type Sink = Arc<Mutex<dyn FnMut(&Event) + Send>>;

/// The devices that are present, and the audit sink for their use.
///
/// Support: Full.
pub struct DeviceHub {
    available: Vec<Box<dyn Transport>>,
    /// Real devices opened through `open_with`. Never removed, like the built-in devices, which
    /// are not offered again once opened.
    in_use: Vec<DeviceId>,
    sink: Sink,
}

impl DeviceHub {
    /// A hub over `devices`. `audit` is called for every open, write and close.
    ///
    /// Support: Full.
    pub fn new(
        devices: Vec<Box<dyn Transport>>,
        audit: impl FnMut(&Event) + Send + 'static,
    ) -> Self {
        DeviceHub {
            available: devices,
            in_use: Vec::new(),
            sink: Arc::new(Mutex::new(audit)),
        }
    }

    /// The identifiers of the present devices that `scope` covers.
    ///
    /// Support: Full.
    pub fn list(&self, scope: &DeviceScope) -> Vec<DeviceId> {
        self.available
            .iter()
            .map(|d| d.id().clone())
            .filter(|id| scope.matches(id))
            .collect()
    }

    /// Opens the device `id`, if the caller's `scope` covers it and it is present and free.
    ///
    /// Support: Full.
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

    /// Opens the device `id` with `options`. A device the hub was built with is used as it is
    /// (its settings are fixed, so `options` are only validated). Otherwise the real device is
    /// opened through the platform (see [`crate::native`]), with the scope checked first and the
    /// open audited like any other. A real device is not offered again while it is open here,
    /// but the hub cannot know that another process has it.
    ///
    /// Support: Partial. Real devices exist only where [`crate::native`] has a backend.
    pub fn open_with(
        &mut self,
        id: &DeviceId,
        scope: &DeviceScope,
        options: &OpenOptions,
    ) -> Result<OpenDevice, DeviceError> {
        options.validate()?;
        if !scope.matches(id) {
            return Err(DeviceError::Denied(format!(
                "{id} is outside the granted scope"
            )));
        }
        if self.available.iter().any(|d| d.id() == id) {
            return self.open(id, scope);
        }
        if self.in_use.iter().any(|d| d == id) {
            return Err(DeviceError::Denied(format!("{id} is already open")));
        }
        let transport = crate::native::open(id, scope, options)?;
        self.in_use.push(id.clone());
        record(&self.sink, EventKind::Open, id, 0);
        Ok(OpenDevice {
            transport,
            sink: self.sink.clone(),
        })
    }

    /// The real devices the platform lists that `scope` covers, plus the hub's own. Nothing is
    /// opened.
    ///
    /// Support: Partial. See [`crate::native::discover`].
    pub fn discover(&self, scope: &DeviceScope) -> Vec<DeviceId> {
        let mut ids = self.list(scope);
        for id in crate::native::discover(scope) {
            if !ids.contains(&id) && !self.in_use.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }
}

/// An open device. Dropping it closes the device, and the close is audited.
///
/// Support: Full.
pub struct OpenDevice {
    transport: Box<dyn Transport>,
    sink: Sink,
}

impl OpenDevice {
    /// Support: Full.
    pub fn id(&self) -> &DeviceId {
        self.transport.id()
    }

    /// Sends `bytes`, and audits the number of bytes that were taken.
    ///
    /// Support: Full.
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

    /// Runs a control transfer, and audits the number of data bytes sent or received.
    ///
    /// Support: Partial. Only raw USB (Linux usbfs) has control transfers; other transports
    /// return [`DeviceError::NotSupported`].
    pub fn control(&mut self, request: &ControlRequest) -> Result<Vec<u8>, DeviceError> {
        let data = self.transport.control(request)?;
        let bytes = if request.direction_in {
            data.len()
        } else {
            request.data.len()
        };
        record(&self.sink, EventKind::Control, self.transport.id(), bytes);
        Ok(data)
    }

    /// Reads up to `max` bytes. Reads are not audited.
    ///
    /// Support: Full.
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
    fn open_with_uses_a_built_in_device_and_checks_the_scope() {
        let mut hub = DeviceHub::new(
            vec![Box::new(VirtualSerial::loopback(port("COM7")))],
            |_| {},
        );
        let scope = DeviceScope::Serial {
            port: "COM*".into(),
        };
        let narrow = DeviceScope::Serial {
            port: "COM1".into(),
        };
        let options = OpenOptions::default();
        assert!(matches!(
            hub.open_with(&port("COM7"), &narrow, &options),
            Err(DeviceError::Denied(_))
        ));
        let bad = OpenOptions {
            timeout_ms: 0,
            ..OpenOptions::default()
        };
        assert!(matches!(
            hub.open_with(&port("COM7"), &scope, &bad),
            Err(DeviceError::Denied(_))
        ));
        assert!(hub.open_with(&port("COM7"), &scope, &options).is_ok());
    }

    #[test]
    fn a_missing_real_device_is_denied_through_open_with() {
        let mut hub = DeviceHub::new(Vec::new(), |_| {});
        let id: DeviceId = "hid:ffff:fffe".parse().expect("id");
        let scope = DeviceScope::Hid {
            vendor: None,
            product: None,
        };
        let result = hub.open_with(&id, &scope, &OpenOptions::default());
        assert!(matches!(
            result,
            Err(DeviceError::Denied(_)) | Err(DeviceError::NotSupported(_))
        ));
    }

    #[test]
    fn control_is_refused_by_a_byte_stream_and_not_audited() {
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let log = events.clone();
        let mut hub = DeviceHub::new(
            vec![Box::new(VirtualSerial::loopback(port("COM7")))],
            move |e| log.lock().expect("log").push(e.clone()),
        );
        let scope = DeviceScope::Serial {
            port: "COM*".into(),
        };
        let mut open = hub.open(&port("COM7"), &scope).expect("open");
        let request = ControlRequest {
            direction_in: true,
            request_type: 0x80,
            request: 6,
            value: 0x0100,
            index: 0,
            data: Vec::new(),
            max_in: 18,
        };
        assert!(matches!(
            open.control(&request),
            Err(DeviceError::NotSupported(_))
        ));
        drop(open);
        let kinds: Vec<EventKind> = events.lock().expect("e").iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![EventKind::Open, EventKind::Close]);
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
