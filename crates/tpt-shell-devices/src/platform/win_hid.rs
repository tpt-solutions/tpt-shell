// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Windows HID devices through SetupAPI and hid.dll (spec §10.2, M7).
//!
//! Listing walks the HID device interfaces and asks each one for its vendor and product ids,
//! without asking for any access, so a keyboard or mouse can be listed. Opening takes the first
//! interface with the requested ids and opens it overlapped, so a read or write never blocks
//! the caller: a read returns what is waiting, and a write that cannot finish returns
//! [`DeviceError::Busy`].
//!
//! The bindings are written by hand, as the serial module's are. The declarations follow
//! `setupapi.h`, `hidsdi.h` and `minwinbase.h`; no HID device was attached when this was written,
//! so the listing is tested on the desktop and the transfers are not (see the docs).
//!
//! Support: Partial. Listing works on Windows. Opening and transfers are built but unverified on
//! hardware: a HID device that the system owns may refuse the open, and a device with no output
//! report refuses writes.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use crate::id::DeviceId;
use crate::transport::{DeviceError, Transport};

type Handle = *mut c_void;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const DIGCF_PRESENT: u32 = 0x0000_0002;
const DIGCF_DEVICEINTERFACE: u32 = 0x0000_0010;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const ERROR_IO_PENDING: i32 = 997;
const ERROR_OPERATION_ABORTED: i32 = 995;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
/// How long a write waits for the device to take its bytes before reporting Busy.
const WRITE_WAIT_MS: u32 = 1000;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Guid {
    pub(super) data1: u32,
    pub(super) data2: u16,
    pub(super) data3: u16,
    pub(super) data4: [u8; 8],
}

#[repr(C)]
pub(super) struct SpDeviceInterfaceData {
    pub(super) cb_size: u32,
    pub(super) interface_class_guid: Guid,
    pub(super) flags: u32,
    pub(super) reserved: usize,
}

#[repr(C)]
struct HiddAttributes {
    size: u32,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
}

#[repr(C)]
pub(super) struct Overlapped {
    pub(super) internal: usize,
    pub(super) internal_high: usize,
    pub(super) offset: u32,
    pub(super) offset_high: u32,
    pub(super) event: Handle,
}

#[link(name = "setupapi")]
unsafe extern "system" {
    fn SetupDiGetClassDevsW(
        class: *const Guid,
        enumerator: *const u16,
        parent: Handle,
        flags: u32,
    ) -> Handle;
    fn SetupDiEnumDeviceInterfaces(
        set: Handle,
        device_info: *const c_void,
        class: *const Guid,
        index: u32,
        interface: *mut SpDeviceInterfaceData,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        set: Handle,
        interface: *const SpDeviceInterfaceData,
        detail: *mut u8,
        detail_size: u32,
        required: *mut u32,
        device_info: *mut c_void,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
}

#[link(name = "hid")]
unsafe extern "system" {
    fn HidD_GetHidGuid(guid: *mut Guid);
    fn HidD_GetAttributes(device: Handle, attributes: *mut HiddAttributes) -> u8;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        disposition: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CreateEventW(
        security: *const c_void,
        manual_reset: i32,
        initial: i32,
        name: *const u16,
    ) -> Handle;
    #[link_name = "ReadFile"]
    fn hid_read_file(
        file: Handle,
        buffer: *mut u8,
        len: u32,
        read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    #[link_name = "WriteFile"]
    fn hid_write_file(
        file: Handle,
        buffer: *const u8,
        len: u32,
        written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn GetOverlappedResult(
        file: Handle,
        overlapped: *const Overlapped,
        transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIo(file: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn CloseHandle(object: Handle) -> i32;
}

fn last_error() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// A NUL-terminated UTF-16 copy of `text`, as the wide-string APIs take it.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Every HID interface the system reports, as its device path and its ids. An interface whose
/// attributes cannot be read is left out, not reported as an error, because listing is a
/// discovery aid.
fn interfaces() -> Result<Vec<(String, DeviceId)>, DeviceError> {
    let mut guid = Guid {
        data1: 0,
        data2: 0,
        data3: 0,
        data4: [0; 8],
    };
    // SAFETY: `guid` is a valid out-pointer for the HID interface class.
    unsafe { HidD_GetHidGuid(&mut guid) };
    // SAFETY: the class GUID is valid for the call; no enumerator or parent window is given.
    let set = unsafe {
        SetupDiGetClassDevsW(
            &guid,
            null_mut(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
    };
    if set == INVALID_HANDLE_VALUE {
        return Err(DeviceError::Io(format!(
            "cannot enumerate HID devices (error {})",
            last_error()
        )));
    }
    let pointer_size = std::mem::size_of::<usize>();
    let detail_size_field: u32 = if pointer_size == 8 { 8 } else { 6 };
    let mut found = Vec::new();
    for index in 0u32.. {
        // SAFETY: a zeroed interface record is a valid buffer; its size field is set first.
        let mut interface = SpDeviceInterfaceData {
            cb_size: std::mem::size_of::<SpDeviceInterfaceData>() as u32,
            interface_class_guid: guid,
            flags: 0,
            reserved: 0,
        };
        // SAFETY: `set` is the list from above and `interface` is a valid out-record.
        let more =
            unsafe { SetupDiEnumDeviceInterfaces(set, null_mut(), &guid, index, &mut interface) };
        if more == 0 {
            // The end of the list (ERROR_NO_MORE_ITEMS) and any other failure both end the walk.
            break;
        }
        let mut required = 0u32;
        // SAFETY: a null buffer with size 0 asks only for the size; the call fails with it set.
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &interface,
                null_mut(),
                0,
                &mut required,
                null_mut(),
            );
        }
        if required == 0 {
            continue;
        }
        let mut detail = vec![0u8; required as usize];
        detail[..4].copy_from_slice(&detail_size_field.to_le_bytes());
        // SAFETY: `detail` holds `required` bytes, and its first field carries the size the API expects.
        let ok = unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &interface,
                detail.as_mut_ptr(),
                required,
                &mut required,
                null_mut(),
            )
        };
        if ok == 0 {
            continue;
        }
        let units: Vec<u16> = detail[4..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .take_while(|&u| u != 0)
            .collect();
        let path = String::from_utf16_lossy(&units);
        if let Some(id) = attributes_of(&path) {
            found.push((path, id));
        }
    }
    // SAFETY: `set` was created above and is destroyed exactly once.
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    Ok(found)
}

/// The vendor and product ids of the interface at `path`, read without any access. `None` when
/// the interface cannot be opened even for queries.
fn attributes_of(path: &str) -> Option<DeviceId> {
    let name = wide(path);
    // SAFETY: the name is NUL-terminated; access 0 asks for no read or write.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut attributes = HiddAttributes {
        size: std::mem::size_of::<HiddAttributes>() as u32,
        vendor_id: 0,
        product_id: 0,
        version_number: 0,
    };
    // SAFETY: `handle` is open, and `attributes` is a valid record of the size it declares.
    let ok = unsafe { HidD_GetAttributes(handle, &mut attributes) };
    // SAFETY: `handle` came from CreateFileW above and is closed once.
    unsafe { CloseHandle(handle) };
    (ok != 0).then_some(DeviceId::Hid {
        vendor: attributes.vendor_id,
        product: attributes.product_id,
    })
}

/// The HID devices present, by vendor and product id, each listed once.
///
/// Support: Partial. Listing works on Windows; several devices with the same ids are one entry.
pub fn list() -> Vec<DeviceId> {
    let mut ids: Vec<DeviceId> = match interfaces() {
        Ok(found) => found.into_iter().map(|(_, id)| id).collect(),
        Err(_) => Vec::new(),
    };
    ids.sort_by_key(|id| id.to_string());
    ids.dedup();
    ids
}

/// Opens the first HID interface with the ids in `id`, overlapped.
///
/// Support: Partial. Built, not verified on a HID device; see the module documentation.
pub fn open(id: &DeviceId) -> Result<WinHid, DeviceError> {
    if !matches!(id, DeviceId::Hid { .. }) {
        return Err(DeviceError::Denied(format!("{id} is not a HID device")));
    }
    let path = interfaces()?
        .into_iter()
        .find(|(_, found)| found == id)
        .map(|(path, _)| path)
        .ok_or_else(|| DeviceError::Denied(format!("{id} is not present")))?;
    let name = wide(&path);
    // Read and write first, then read only: some devices refuse writes from a second program.
    for access in [GENERIC_READ | GENERIC_WRITE, GENERIC_READ] {
        // SAFETY: the name is NUL-terminated; the handle is overlapped, so transfers never block.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                access,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Ok(WinHid {
                id: id.clone(),
                handle,
            });
        }
    }
    Err(DeviceError::Denied(format!(
        "{id} cannot be opened (error {})",
        last_error()
    )))
}

/// An open HID device. Dropping it closes the handle.
///
/// Support: Partial. See the module documentation.
pub struct WinHid {
    id: DeviceId,
    handle: Handle,
}

// SAFETY: the handle is owned by this value and closed once, in Drop. Every operation takes
// `&mut self`, so one thread at a time uses it.
unsafe impl Send for WinHid {}

impl Drop for WinHid {
    fn drop(&mut self) {
        // SAFETY: the handle is closed here and nowhere else.
        unsafe { CloseHandle(self.handle) };
    }
}

/// A manual-reset event for one overlapped operation, closed when dropped.
struct Event(Handle);

impl Event {
    fn new() -> Result<Self, DeviceError> {
        // SAFETY: an unnamed, manual-reset, non-signalled event with default security.
        let handle = unsafe { CreateEventW(null_mut(), 1, 0, null_mut()) };
        if handle.is_null() {
            return Err(DeviceError::Io(format!(
                "cannot create an event (error {})",
                last_error()
            )));
        }
        Ok(Event(handle))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the event was created by Event::new and is closed here only.
        unsafe { CloseHandle(self.0) };
    }
}

fn overlapped(event: &Event) -> Overlapped {
    Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: event.0,
    }
}

impl Transport for WinHid {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let event = Event::new()?;
        let mut op = overlapped(&event);
        let mut written = 0u32;
        // SAFETY: `bytes` and `op` stay alive until the operation is finished or cancelled below.
        let ok = unsafe {
            hid_write_file(
                self.handle,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                (&mut op as *mut Overlapped).cast(),
            )
        };
        if ok != 0 {
            return Ok(());
        }
        let error = last_error();
        if error != ERROR_IO_PENDING {
            return Err(DeviceError::Io(format!("write failed (error {error})")));
        }
        // SAFETY: the event belongs to this operation and stays open for the wait.
        if unsafe { WaitForSingleObject(event.0, WRITE_WAIT_MS) } == WAIT_OBJECT_0 {
            // SAFETY: the operation has signalled, so reading its result does not wait.
            return match unsafe { GetOverlappedResult(self.handle, &op, &mut written, 0) } {
                0 => Err(DeviceError::Io(format!(
                    "write failed (error {})",
                    last_error()
                ))),
                _ => Ok(()),
            };
        }
        // The device did not take the bytes in time. Cancel, and wait for the cancellation so
        // the buffers are no longer in use before they go out of scope.
        // SAFETY: the handle is open, and the pending operation belongs to it.
        unsafe {
            CancelIo(self.handle);
            GetOverlappedResult(self.handle, &op, &mut written, 1);
        }
        Err(DeviceError::Busy)
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        if max == 0 {
            return Ok(Vec::new());
        }
        let event = Event::new()?;
        let mut op = overlapped(&event);
        let mut buffer = vec![0u8; max];
        let mut got = 0u32;
        // SAFETY: `buffer` and `op` stay alive until the operation is finished or cancelled below.
        let ok = unsafe {
            hid_read_file(
                self.handle,
                buffer.as_mut_ptr(),
                max as u32,
                &mut got,
                (&mut op as *mut Overlapped).cast(),
            )
        };
        if ok == 0 {
            let error = last_error();
            if error != ERROR_IO_PENDING {
                return Err(DeviceError::Io(format!("read failed (error {error})")));
            }
            // SAFETY: the event belongs to this operation.
            let signalled = unsafe { WaitForSingleObject(event.0, 0) };
            if signalled != WAIT_OBJECT_0 && signalled != WAIT_TIMEOUT {
                return Err(DeviceError::Io(format!(
                    "read wait failed (error {})",
                    last_error()
                )));
            }
            if signalled == WAIT_TIMEOUT {
                // Nothing is waiting. Cancel the read; a report that arrives while cancelling is
                // still delivered by the result call below.
                // SAFETY: the handle is open, and the pending read belongs to it.
                unsafe { CancelIo(self.handle) };
            }
            // SAFETY: the operation is finished or cancelled, so waiting for its result returns.
            let done = unsafe { GetOverlappedResult(self.handle, &op, &mut got, 1) };
            if done == 0 {
                let error = last_error();
                if error == ERROR_OPERATION_ABORTED {
                    return Ok(Vec::new());
                }
                return Err(DeviceError::Io(format!("read failed (error {error})")));
            }
        }
        buffer.truncate(got as usize);
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Listing reads the present HID interfaces. Every entry is a HID id; a machine with no HID
    /// device lists nothing, which is also a pass.
    #[test]
    fn listed_devices_are_hid_ids() {
        for id in list() {
            assert!(matches!(id, DeviceId::Hid { .. }), "{id}");
        }
    }

    #[test]
    fn an_absent_device_is_refused_not_faked() {
        let id = DeviceId::Hid {
            vendor: 0xffff,
            product: 0xfffe,
        };
        assert!(matches!(open(&id), Err(DeviceError::Denied(_))));
    }

    /// Opening a present device may be refused, since the system often owns input devices. When
    /// it opens, a read must return at once, with or without data, and never block.
    #[test]
    fn a_present_device_reads_without_blocking_when_it_opens() {
        let Some(id) = list().into_iter().next() else {
            return;
        };
        let Ok(mut device) = open(&id) else {
            eprintln!("skipped: {id} is owned by the system and cannot be opened");
            return;
        };
        let started = Instant::now();
        let read = device.read(64);
        assert!(read.is_ok(), "{read:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "read blocked");
    }
}
