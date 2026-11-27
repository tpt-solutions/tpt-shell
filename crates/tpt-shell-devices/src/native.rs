// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Real devices, found and opened through the operating system (spec §10.2, M7). The hub uses
//! this when a device is not one of the transports it was built with.
//!
//! What exists per platform:
//!
//! | | Windows | macOS | Linux |
//! |---|---|---|---|
//! | serial | open (COM), discovery from the SERIALCOMM registry key | termios, `/dev` discovery | termios, `/dev` discovery |
//! | HID | listing and open through SetupAPI (open not verified on hardware) | IOKit | hidraw, sysfs discovery |
//! | USB raw | WinUSB, best effort (transfers unverified) | unsupported (IOKit USB not built) | usbfs, sysfs discovery |
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
#[cfg(target_os = "windows")]
fn hid_list() -> Vec<DeviceId> {
    crate::platform::win_hid::list()
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
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
#[cfg(target_os = "windows")]
fn hid_open(id: &DeviceId) -> Result<Box<dyn Transport>, DeviceError> {
    Ok(Box::new(crate::platform::win_hid::open(id)?))
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
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
#[cfg(windows)]
mod usb_impl {
    use super::*;
    pub fn list() -> Vec<DeviceId> {
        crate::platform::win_usb::list()
    }
    pub fn open(id: &DeviceId, o: &OpenOptions) -> Result<Box<dyn Transport>, DeviceError> {
        Ok(Box::new(crate::platform::win_usb::open(id, o)?))
    }
}
#[cfg(not(any(
    windows,
    all(
        target_os = "linux",
        not(any(
            target_arch = "mips",
            target_arch = "mips64",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "sparc",
            target_arch = "sparc64"
        ))
    )
)))]
mod usb_impl {
    use super::*;
    pub fn list() -> Vec<DeviceId> {
        Vec::new()
    }
    pub fn open(_id: &DeviceId, _o: &OpenOptions) -> Result<Box<dyn Transport>, DeviceError> {
        Err(DeviceError::NotSupported(
            "raw USB on this platform (usbfs is Linux only; IOKit USB is not built)",
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

    /// On a platform with no HID backend, opening is refused as unsupported. Where a backend
    /// exists (Windows, Linux, macOS), an id that is not present is refused rather than faked.
    #[test]
    fn hid_open_of_an_absent_device_is_refused_not_faked() {
        let id: DeviceId = "hid:1a2b:0001".parse().expect("id");
        let scope = DeviceScope::Hid {
            vendor: None,
            product: None,
        };
        let refused = open(&id, &scope, &OpenOptions::default());
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        assert!(matches!(refused, Err(DeviceError::NotSupported(_))));
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        assert!(refused.is_err());
    }

    /// Where no USB backend exists, opening is refused as unsupported. Where one exists (Linux,
    /// Windows through WinUSB), an id that is not present is refused rather than faked.
    #[test]
    fn usb_open_of_an_absent_device_is_refused_not_faked() {
        let id: DeviceId = "usb:0403:6001".parse().expect("id");
        let scope = DeviceScope::Usb {
            vendor: None,
            product: None,
        };
        let refused = open(&id, &scope, &OpenOptions::default());
        #[cfg(not(any(target_os = "linux", windows)))]
        assert!(matches!(refused, Err(DeviceError::NotSupported(_))));
        #[cfg(any(target_os = "linux", windows))]
        assert!(refused.is_err());
    }
}
