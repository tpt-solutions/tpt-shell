// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! HID devices on Linux through hidraw (spec §10.2, M7).
//!
//! Devices are found under `/sys/class/hidraw`, whose `uevent` files give the vendor and product
//! (parsed by [`crate::descriptors::parse_hid_uevent`]). A device is opened at `/dev/hidrawN`
//! non-blocking. One `read` returns one input report; a `write` sends one output report whose
//! first byte is the report ID (0 when the device uses no numbered reports), which is how the
//! kernel interface works. Ordinary users usually cannot open these nodes until a udev rule
//! grants access (see the devices guide).
//!
//! Support: Partial. Compile-checked from a Windows host; the runtime test runs only when a
//! hidraw device is present and readable.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;

use crate::descriptors::{parse_hid_uevent, HidInfo};
use crate::id::DeviceId;
use crate::transport::{DeviceError, Transport};

/// Largest input report one read returns.
const MAX_REPORT: usize = 4096;

/// An open hidraw device. Dropping it closes the node.
///
/// Support: Partial. Compile-checked; run only where a device is present.
pub struct HidrawDevice {
    id: DeviceId,
    file: File,
}

/// A hidraw node name: `hidraw` and digits only, so a path cannot be built from it.
fn valid_node(name: &str) -> bool {
    name.strip_prefix("hidraw")
        .is_some_and(|n| !n.is_empty() && n.len() <= 4 && n.chars().all(|c| c.is_ascii_digit()))
}

/// Every hidraw node with what sysfs says about it.
fn nodes() -> Vec<(String, HidInfo)> {
    let Ok(entries) = std::fs::read_dir("/sys/class/hidraw") else {
        return Vec::new();
    };
    let mut found: Vec<(String, HidInfo)> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| valid_node(name))
        .filter_map(|name| {
            let text =
                std::fs::read_to_string(format!("/sys/class/hidraw/{name}/device/uevent")).ok()?;
            Some((name, parse_hid_uevent(&text)?))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// The HID devices present, once per vendor and product.
///
/// Support: Partial. Compile-checked; run only where devices are present.
pub fn list() -> Vec<DeviceId> {
    let mut ids: Vec<DeviceId> = Vec::new();
    for (_, info) in nodes() {
        let id = info.id();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// Opens the first hidraw node whose vendor and product match `id`.
///
/// Support: Partial. Compile-checked; run only where a device is present.
pub fn open(id: &DeviceId) -> Result<HidrawDevice, DeviceError> {
    let DeviceId::Hid { vendor, product } = id else {
        return Err(DeviceError::Denied(format!("{id} is not a HID device")));
    };
    let mut last_error = None;
    for (name, info) in nodes() {
        if info.vendor != *vendor || info.product != *product {
            continue;
        }
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(format!("/dev/{name}"))
        {
            Ok(file) => {
                return Ok(HidrawDevice {
                    id: id.clone(),
                    file,
                })
            }
            Err(e) => last_error = Some((name, e)),
        }
    }
    Err(match last_error {
        Some((name, e)) if e.kind() == io::ErrorKind::PermissionDenied => DeviceError::Denied(
            format!("no permission to open /dev/{name}; a udev rule must grant access"),
        ),
        Some((name, e)) => DeviceError::Io(format!("cannot open /dev/{name}: {e}")),
        None => DeviceError::Denied(format!("{id} is not present")),
    })
}

impl Transport for HidrawDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        if bytes.is_empty() {
            return Ok(());
        }
        // One write is one report. A short write would send a truncated report, so it is an error.
        match self.file.write(bytes) {
            Ok(n) if n == bytes.len() => Ok(()),
            Ok(n) => Err(DeviceError::Io(format!(
                "hidraw took {n} of {} bytes of the report",
                bytes.len()
            ))),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Err(DeviceError::Busy),
            Err(e) => Err(DeviceError::Io(format!("cannot write the report: {e}"))),
        }
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut buf = vec![0u8; max.min(MAX_REPORT)];
        match self.file.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(Vec::new())
            }
            Err(e) => Err(DeviceError::Io(format!("cannot read a report: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hidraw_node_names_are_used() {
        assert!(valid_node("hidraw0"));
        assert!(valid_node("hidraw12"));
        for bad in ["hidraw", "hidraw0x", "hidraw../x", "hiddev0", "hidraw12345"] {
            assert!(!valid_node(bad), "{bad}");
        }
    }

    #[test]
    fn a_device_that_is_absent_is_denied() {
        let id: DeviceId = "hid:ffff:fffe".parse().expect("id");
        assert!(matches!(open(&id), Err(DeviceError::Denied(_))));
    }

    #[test]
    fn a_present_device_reads_without_blocking() {
        let Some(id) = list().into_iter().next() else {
            eprintln!("skipped: no hidraw device is present on this machine");
            return;
        };
        match open(&id) {
            Ok(mut device) => {
                let report = device.read(64).expect("a read never blocks");
                assert!(report.len() <= 64);
            }
            Err(DeviceError::Denied(why)) => {
                eprintln!("skipped: {why}");
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
