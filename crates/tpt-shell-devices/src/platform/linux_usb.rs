// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Raw USB on Linux through usbfs (spec §10.2, M7).
//!
//! Devices are found under `/sys/bus/usb/devices` and opened at `/dev/bus/usb/BBB/DDD`. The
//! descriptors the node returns on read give the interface and its bulk endpoints
//! ([`crate::descriptors::parse_usb_descriptors`]). `write` and `read` are bulk transfers on the
//! first bulk OUT and IN endpoints of the claimed interface; `control` is a control transfer on
//! endpoint 0. If another driver owns the interface it cannot be claimed: control transfers still
//! work, and bulk transfers return a denial that says why.
//!
//! A read waits at most [`READ_POLL_MS`] and returns nothing on a timeout, so it suits devices
//! that answer promptly; a transfer that times out is cancelled by the kernel.
//!
//! The ioctl numbers use the generic Linux encoding, so this module is built only on
//! architectures that use it (not MIPS, PowerPC or SPARC).
//!
//! Support: Partial. Compile-checked from a Windows host; the runtime test runs only when a USB
//! device is present and its node is accessible.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

use crate::descriptors::{
    ioc, parse_sysfs_hex_u16, parse_usb_descriptors, usbfs_path, InterfaceInfo, UsbInfo,
};
use crate::id::DeviceId;
use crate::options::OpenOptions as DeviceOpenOptions;
use crate::transport::{ControlRequest, DeviceError, Transport};

/// How long one read waits for the device, in milliseconds.
const READ_POLL_MS: u32 = 10;
/// Most bytes in one bulk ioctl.
const BULK_CHUNK: usize = 16 * 1024;

/// `struct usbdevfs_ctrltransfer`.
#[repr(C)]
struct CtrlTransfer {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
    timeout: u32,
    data: *mut c_void,
}

/// `struct usbdevfs_bulktransfer`.
#[repr(C)]
struct BulkTransfer {
    endpoint: u32,
    length: u32,
    timeout: u32,
    data: *mut c_void,
}

const USBDEVFS_CONTROL: u32 = ioc(3, b'U', 0, std::mem::size_of::<CtrlTransfer>());
const USBDEVFS_BULK: u32 = ioc(3, b'U', 2, std::mem::size_of::<BulkTransfer>());
const USBDEVFS_CLAIMINTERFACE: u32 = ioc(2, b'U', 15, std::mem::size_of::<u32>());
const USBDEVFS_RELEASEINTERFACE: u32 = ioc(2, b'U', 16, std::mem::size_of::<u32>());

/// An open USB device. Dropping it releases the interface and closes the node.
///
/// Support: Partial. Compile-checked; run only where a device is present.
pub struct UsbfsDevice {
    id: DeviceId,
    file: File,
    interface: InterfaceInfo,
    claimed: bool,
    timeout_ms: u32,
}

struct Present {
    bus: u16,
    device: u16,
    vendor: u16,
    product: u16,
}

fn read_attr(dir: &str, name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{dir}/{name}")).ok()
}

fn present() -> Vec<Present> {
    let Ok(entries) = std::fs::read_dir("/sys/bus/usb/devices") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(dir) = entry.path().to_str().map(str::to_string) else {
            continue;
        };
        // Interfaces (`1-1:1.0`) have no idVendor, so they are skipped here.
        let parse = || -> Option<Present> {
            Some(Present {
                bus: read_attr(&dir, "busnum")?.trim().parse().ok()?,
                device: read_attr(&dir, "devnum")?.trim().parse().ok()?,
                vendor: parse_sysfs_hex_u16(&read_attr(&dir, "idVendor")?)?,
                product: parse_sysfs_hex_u16(&read_attr(&dir, "idProduct")?)?,
            })
        };
        if let Some(p) = parse() {
            found.push(p);
        }
    }
    found.sort_by_key(|p| (p.bus, p.device));
    found
}

/// The USB devices present, once per vendor and product. Hubs are included; the scope decides
/// what the caller may see.
///
/// Support: Partial. Compile-checked; run only where devices are present.
pub fn list() -> Vec<DeviceId> {
    let mut ids: Vec<DeviceId> = Vec::new();
    for p in present() {
        let id = DeviceId::Usb {
            vendor: p.vendor,
            product: p.product,
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Opens the first device matching `id`, with the interface and timeout from `options`.
///
/// Support: Partial. Compile-checked; run only where a device is present.
pub fn open(id: &DeviceId, options: &DeviceOpenOptions) -> Result<UsbfsDevice, DeviceError> {
    let DeviceId::Usb { vendor, product } = id else {
        return Err(DeviceError::Denied(format!("{id} is not a USB device")));
    };
    let Some(found) = present()
        .into_iter()
        .find(|p| p.vendor == *vendor && p.product == *product)
    else {
        return Err(DeviceError::Denied(format!("{id} is not present")));
    };
    let path = usbfs_path(found.bus, found.device);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => DeviceError::Denied(format!(
                "no permission to open {path}; a udev rule must grant access"
            )),
            _ => DeviceError::Io(format!("cannot open {path}: {e}")),
        })?;
    let mut raw = Vec::new();
    file.by_ref()
        .take(4096)
        .read_to_end(&mut raw)
        .map_err(|e| DeviceError::Io(format!("cannot read the descriptors of {path}: {e}")))?;
    let info: UsbInfo = parse_usb_descriptors(&raw)
        .ok_or_else(|| DeviceError::Io(format!("{path} returned malformed descriptors")))?;
    let interface = info
        .interfaces
        .into_iter()
        .find(|i| i.number == options.usb_interface)
        .unwrap_or(InterfaceInfo {
            number: options.usb_interface,
            bulk_in: None,
            bulk_out: None,
        });
    let number = u32::from(interface.number);
    // SAFETY: the descriptor is open, and CLAIMINTERFACE takes a pointer to a u32 that lives for
    // the call.
    let claimed = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            USBDEVFS_CLAIMINTERFACE as _,
            &number as *const u32,
        )
    } == 0;
    Ok(UsbfsDevice {
        id: id.clone(),
        file,
        interface,
        claimed,
        timeout_ms: options.timeout_ms,
    })
}

impl UsbfsDevice {
    fn bulk(&self, endpoint: u8, data: *mut u8, len: usize, timeout: u32) -> Result<usize, i32> {
        let mut transfer = BulkTransfer {
            endpoint: u32::from(endpoint),
            length: len as u32,
            timeout,
            data: data.cast(),
        };
        // SAFETY: `transfer` is a valid usbdevfs_bulktransfer, and `data` is valid for `len`
        // bytes (the callers pass a live slice or Vec) for the duration of the call.
        let n = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                USBDEVFS_BULK as _,
                &mut transfer as *mut BulkTransfer,
            )
        };
        if n < 0 {
            Err(errno())
        } else {
            Ok(n as usize)
        }
    }

    fn need_claim(&self) -> Result<(), DeviceError> {
        if self.claimed {
            Ok(())
        } else {
            Err(DeviceError::Denied(format!(
                "interface {} is owned by another driver, so bulk transfers are refused",
                self.interface.number
            )))
        }
    }
}

fn map_errno(code: i32, what: &str) -> DeviceError {
    match code {
        libc::ENODEV | libc::ESHUTDOWN => DeviceError::Closed,
        libc::ETIMEDOUT => DeviceError::Busy,
        _ => DeviceError::Io(format!("{what}: {}", io::Error::from_raw_os_error(code))),
    }
}

impl Transport for UsbfsDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        self.need_claim()?;
        let endpoint = self.interface.bulk_out.ok_or(DeviceError::NotSupported(
            "bulk OUT (the interface has no bulk OUT endpoint)",
        ))?;
        let mut buf = bytes.to_vec();
        let mut sent = 0;
        while sent < buf.len() {
            let len = (buf.len() - sent).min(BULK_CHUNK);
            // `sent` is within `buf`, so the pointer stays inside the allocation.
            let ptr = buf[sent..].as_mut_ptr();
            match self.bulk(endpoint, ptr, len, self.timeout_ms) {
                Ok(0) => return Err(DeviceError::Closed),
                Ok(n) => sent += n,
                Err(libc::ETIMEDOUT) if sent > 0 => {
                    return Err(DeviceError::Io(format!(
                        "bulk write timed out after {sent} of {} bytes",
                        bytes.len()
                    )))
                }
                Err(code) => return Err(map_errno(code, "cannot write to the device")),
            }
        }
        Ok(())
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        self.need_claim()?;
        let endpoint = self.interface.bulk_in.ok_or(DeviceError::NotSupported(
            "bulk IN (the interface has no bulk IN endpoint)",
        ))?;
        let mut buf = vec![0u8; max.min(BULK_CHUNK)];
        if buf.is_empty() {
            return Ok(buf);
        }
        match self.bulk(endpoint, buf.as_mut_ptr(), buf.len(), READ_POLL_MS) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(libc::ETIMEDOUT) => Ok(Vec::new()),
            Err(code) => Err(map_errno(code, "cannot read from the device")),
        }
    }

    fn control(&mut self, request: &ControlRequest) -> Result<Vec<u8>, DeviceError> {
        // The direction bit of the request type must agree with the direction asked for.
        if (request.request_type & 0x80 != 0) != request.direction_in {
            return Err(DeviceError::Denied(
                "request_type direction bit disagrees with direction_in".to_string(),
            ));
        }
        let mut buf = if request.direction_in {
            vec![0u8; usize::from(request.max_in)]
        } else {
            if request.data.len() > usize::from(u16::MAX) {
                return Err(DeviceError::Denied(
                    "control data longer than 65535 bytes".to_string(),
                ));
            }
            request.data.clone()
        };
        let mut transfer = CtrlTransfer {
            request_type: request.request_type,
            request: request.request,
            value: request.value,
            index: request.index,
            length: buf.len() as u16,
            timeout: self.timeout_ms,
            data: buf.as_mut_ptr().cast(),
        };
        // SAFETY: `transfer` is a valid usbdevfs_ctrltransfer. `data` points into `buf`, which is
        // valid for `length` bytes and outlives the call.
        let n = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                USBDEVFS_CONTROL as _,
                &mut transfer as *mut CtrlTransfer,
            )
        };
        if n < 0 {
            return Err(map_errno(errno(), "control transfer failed"));
        }
        if request.direction_in {
            buf.truncate(n as usize);
            Ok(buf)
        } else {
            Ok(Vec::new())
        }
    }
}

impl Drop for UsbfsDevice {
    fn drop(&mut self) {
        if self.claimed {
            let number = u32::from(self.interface.number);
            // SAFETY: the descriptor is still open, and RELEASEINTERFACE takes a pointer to a u32
            // that lives for the call. A failure is ignored: closing the node releases it anyway.
            unsafe {
                libc::ioctl(
                    self.file.as_raw_fd(),
                    USBDEVFS_RELEASEINTERFACE as _,
                    &number as *const u32,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_use_the_struct_sizes() {
        // On 64-bit targets both structs are 24 bytes: 0xc0185500 and 0xc0185502.
        if std::mem::size_of::<usize>() == 8 {
            assert_eq!(USBDEVFS_CONTROL, 0xc018_5500);
            assert_eq!(USBDEVFS_BULK, 0xc018_5502);
        }
        assert_eq!(USBDEVFS_CLAIMINTERFACE, 0x8004_550f);
        assert_eq!(USBDEVFS_RELEASEINTERFACE, 0x8004_5510);
    }

    #[test]
    fn a_device_that_is_absent_is_denied() {
        let id: DeviceId = "usb:ffff:fffe".parse().expect("id");
        assert!(matches!(
            open(&id, &DeviceOpenOptions::default()),
            Err(DeviceError::Denied(_))
        ));
    }

    #[test]
    fn a_present_device_answers_a_descriptor_request() {
        let ids = list();
        if ids.is_empty() {
            eprintln!("skipped: no USB device is present on this machine");
            return;
        }
        for id in ids {
            match open(&id, &DeviceOpenOptions::default()) {
                Ok(mut device) => {
                    // GET_DESCRIPTOR(device): standard, device-to-host, 18 bytes.
                    let request = ControlRequest {
                        direction_in: true,
                        request_type: 0x80,
                        request: 6,
                        value: 0x0100,
                        index: 0,
                        data: Vec::new(),
                        max_in: 18,
                    };
                    match device.control(&request) {
                        Ok(bytes) => {
                            assert_eq!(bytes.len(), 18);
                            assert_eq!(bytes[1], 1);
                            return;
                        }
                        Err(e) => eprintln!("{id}: control failed: {e}"),
                    }
                }
                Err(e) => eprintln!("{id}: skipped: {e}"),
            }
        }
        eprintln!("skipped: no present device could be opened (permissions)");
    }
}
