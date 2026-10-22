// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Real devices, found and opened through the operating system (spec §10.2, M7). The hub uses
//! this when a device is not one of the transports it was built with.
//!
//! What exists per platform:
//!
//! | | Windows | macOS | Linux |
//! |---|---|---|---|
//! | serial | open (COM), no discovery | termios, `/dev` discovery | termios, `/dev` discovery |
//! | HID | unsupported | IOKit | hidraw, sysfs discovery |
//! | USB raw | unsupported (WinUSB not built) | unsupported (IOKit USB not built) | usbfs, sysfs discovery |
//!
//! Anything marked unsupported returns [`DeviceError::NotSupported`] naming the gap; no call
//! pretends to succeed. Network devices are not opened here; see [`crate::net`].

use crate::id::{DeviceId, DeviceScope};
use crate::options::OpenOptions;
use crate::transport::{DeviceError, Transport};

/// The real devices present that `scope` covers. Nothing is opened. Kinds without a platform
/// listing contribute nothing, and a scope never lists another kind.
///
/// Support: Partial. See the table in the module documentation.
pub fn discover(scope: &DeviceScope) -> Vec<DeviceId> {
    let all: Vec<DeviceId> = match scope {
        DeviceScope::Serial { .. } => crate::serial::list(),
        DeviceScope::Hid { .. } => hid_list(),
        DeviceScope::Usb { .. } => usb_list(),
        DeviceScope::Net { .. } => Vec::new(),
    };
    all.into_iter().filter(|id| scope.matches(id)).collect()
}

/// Opens the real device `id` if `scope` covers it.
///
/// Support: Partial. See the table in the module documentation.
pub fn open(
    id: &DeviceId,
    scope: &DeviceScope,
    options: &OpenOptions,
) -> Result<Box<dyn Transport>, DeviceError> {
    if !scope.matches(id) {
        return Err(DeviceError::Denied(format!(
            "{id} is outside the granted scope"
        )));
    }
    options.validate()?;
    match id {
        DeviceId::Serial { .. } => crate::serial::open_with(scope, id, &options.serial),
        DeviceId::Hid { .. } => hid_open(id),
        DeviceId::Usb { .. } => usb_open(id, options),
        DeviceId::Net { .. } => Err(DeviceError::NotSupported(
            "network devices through the hub (use the net module)",
        )),
    }
}

#[cfg(target_os = "linux")]
fn hid_list() -> Vec<DeviceId> {
    crate::platform::linux_hid::list()
}
#[cfg(target_os = "macos")]
fn hid_list() -> Vec<DeviceId> {
    crate::platform::macos_hid::list()
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn hid_list() -> Vec<DeviceId> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn hid_open(id: &DeviceId) -> Result<Box<dyn Transport>, DeviceError> {
    Ok(Box::new(crate::platform::linux_hid::open(id)?))
}
#[cfg(target_os = "macos")]
fn hid_open(id: &DeviceId) -> Result<Box<dyn Transport>, DeviceError> {
    Ok(Box::new(crate::platform::macos_hid::open(id)?))
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn hid_open(_id: &DeviceId) -> Result<Box<dyn Transport>, DeviceError> {
    Err(DeviceError::NotSupported("HID on this platform"))
}

#[cfg(all(
    target_os = "linux",
    not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "sparc",
        target_arch = "sparc64"
    ))
))]
mod usb_impl {
    use super::*;
    pub fn list() -> Vec<DeviceId> {
        crate::platform::linux_usb::list()
    }
    pub fn open(id: &DeviceId, o: &OpenOptions) -> Result<Box<dyn Transport>, DeviceError> {
        Ok(Box::new(crate::platform::linux_usb::open(id, o)?))
    }
}
#[cfg(not(all(
    target_os = "linux",
    not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "sparc",
        target_arch = "sparc64"
    ))
)))]
mod usb_impl {
    use super::*;
    pub fn list() -> Vec<DeviceId> {
        Vec::new()
    }
    pub fn open(_id: &DeviceId, _o: &OpenOptions) -> Result<Box<dyn Transport>, DeviceError> {
        Err(DeviceError::NotSupported(
            "raw USB on this platform (usbfs is Linux only; WinUSB and IOKit USB are not built)",
        ))
    }
}

fn usb_list() -> Vec<DeviceId> {
    usb_impl::list()
}
fn usb_open(id: &DeviceId, options: &OpenOptions) -> Result<Box<dyn Transport>, DeviceError> {
    usb_impl::open(id, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_that_does_not_cover_the_device_is_refused() {
        let id: DeviceId = "hid:1a2b:0001".parse().expect("id");
        let scope = DeviceScope::Hid {
            vendor: Some(0x0001),
            product: None,
        };
        assert!(matches!(
            open(&id, &scope, &OpenOptions::default()),
            Err(DeviceError::Denied(_))
        ));
    }

    #[test]
    fn network_devices_are_not_opened_here() {
        let id: DeviceId = "net:printer.local:9100".parse().expect("id");
        let scope = DeviceScope::Net {
            host: "*".into(),
            port: None,
        };
        assert!(matches!(
            open(&id, &scope, &OpenOptions::default()),
            Err(DeviceError::NotSupported(_))
        ));
        assert!(discover(&scope).is_empty());
    }

    #[test]
    fn discovery_stays_inside_the_scope() {
        let scope = DeviceScope::Hid {
            vendor: Some(0xffff),
            product: Some(0xfffe),
        };
        assert!(discover(&scope).is_empty());
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    #[test]
    fn hid_is_unsupported_not_faked_on_this_platform() {
        let id: DeviceId = "hid:1a2b:0001".parse().expect("id");
        let scope = DeviceScope::Hid {
            vendor: None,
            product: None,
        };
        assert!(matches!(
            open(&id, &scope, &OpenOptions::default()),
            Err(DeviceError::NotSupported(_))
        ));
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn usb_is_unsupported_not_faked_off_linux() {
        let id: DeviceId = "usb:0403:6001".parse().expect("id");
        let scope = DeviceScope::Usb {
            vendor: None,
            product: None,
        };
        assert!(matches!(
            open(&id, &scope, &OpenOptions::default()),
            Err(DeviceError::NotSupported(_))
        ));
    }
}
