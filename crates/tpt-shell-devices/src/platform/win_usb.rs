// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Raw USB on Windows through WinUSB (spec §10.2, M7). Best effort: this was written without a
//! device bound to the WinUSB driver, so nothing here has moved bytes to a real device. The tests
//! check what can run without one (the listing and the refusals); see the docs.
//!
//! A device is listed when its driver service is `WinUSB`. Its interface paths come from the
//! `DeviceInterfaceGUIDs` value of its device key. Opening takes the first interface path, opens
//! it overlapped, and finds the bulk endpoints of the chosen interface. Bulk transfers and
//! control transfers run overlapped and wait a bounded time, so a stalled device returns
//! [`DeviceError::Busy`] or an error instead of hanging the caller.
//!
//! The bindings are written by hand. The SetupAPI, overlapped and file declarations are shared
//! with the HID module (same types, so the compiler sees one declaration per symbol); the WinUSB
//! and registry ones are declared here, following `winusb.h` and `setupapi.h`.
//!
//! Support: Partial. Built and compile-checked; listing and refusals are tested. Transfers are not
//! verified on hardware.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use super::win_hid::{Guid, Overlapped, SpDeviceInterfaceData};
use crate::id::DeviceId;
use crate::options::OpenOptions;
use crate::transport::{ControlRequest, DeviceError, Transport};

type Handle = *mut c_void;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const DIGCF_PRESENT: u32 = 0x0000_0002;
const DIGCF_ALLCLASSES: u32 = 0x0000_0004;
const SPDRP_HARDWAREID: u32 = 0x0000_0001;
const SPDRP_SERVICE: u32 = 0x0000_0004;
const DICS_FLAG_GLOBAL: u32 = 0x0000_0001;
const DIREG_DEV: u32 = 0x0000_0001;
const KEY_READ: u32 = 0x0002_0019;
const REG_MULTI_SZ: u32 = 7;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const ERROR_IO_PENDING: i32 = 997;
const WAIT_OBJECT_0: u32 = 0;
/// The usual bulk pipe type in `WINUSB_PIPE_INFORMATION` (`UsbdPipeTypeBulk`).
const PIPE_TYPE_BULK: i32 = 2;
/// How long a transfer waits before it is cancelled and reported.
const TRANSFER_WAIT_MS: u32 = 1000;

#[repr(C)]
struct SpDevinfoData {
    cb_size: u32,
    class_guid: Guid,
    dev_inst: u32,
    reserved: usize,
}

#[repr(C)]
#[derive(Default)]
struct UsbInterfaceDescriptor {
    length: u8,
    descriptor_type: u8,
    interface_number: u8,
    alternate_setting: u8,
    num_endpoints: u8,
    interface_class: u8,
    interface_sub_class: u8,
    interface_protocol: u8,
    interface: u8,
}

#[repr(C)]
#[derive(Default)]
struct WinUsbPipeInformation {
    pipe_type: i32,
    pipe_id: u8,
    maximum_packet_size: u16,
    interval: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WinUsbSetupPacket {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
}

#[link(name = "setupapi")]
unsafe extern "system" {
    fn SetupDiGetClassDevsW(
        class: *const Guid,
        enumerator: *const u16,
        parent: Handle,
        flags: u32,
    ) -> Handle;
    fn SetupDiEnumDeviceInfo(set: Handle, index: u32, info: *mut SpDevinfoData) -> i32;
    fn SetupDiGetDeviceRegistryPropertyW(
        set: Handle,
        info: *const SpDevinfoData,
        property: u32,
        property_type: *mut u32,
        buffer: *mut u8,
        buffer_size: u32,
        required: *mut u32,
    ) -> i32;
    fn SetupDiOpenDevRegKey(
        set: Handle,
        info: *const SpDevinfoData,
        scope: u32,
        hardware_profile: u32,
        key_type: u32,
        desired: u32,
    ) -> isize;
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

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegQueryValueExW(
        key: isize,
        name: *const u16,
        reserved: *mut u32,
        value_type: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

#[link(name = "winusb")]
unsafe extern "system" {
    fn WinUsb_Initialize(device: Handle, interface: *mut Handle) -> i32;
    fn WinUsb_Free(interface: Handle) -> i32;
    fn WinUsb_GetAssociatedInterface(interface: Handle, index: u8, associated: *mut Handle) -> i32;
    fn WinUsb_QueryInterfaceSettings(
        interface: Handle,
        alternate: u8,
        descriptor: *mut UsbInterfaceDescriptor,
    ) -> i32;
    fn WinUsb_QueryPipe(
        interface: Handle,
        alternate: u8,
        index: u8,
        information: *mut WinUsbPipeInformation,
    ) -> i32;
    #[link_name = "WinUsb_ReadPipe"]
    fn usb_read_pipe(
        interface: Handle,
        pipe: u8,
        buffer: *mut u8,
        len: u32,
        transferred: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    #[link_name = "WinUsb_WritePipe"]
    fn usb_write_pipe(
        interface: Handle,
        pipe: u8,
        buffer: *const u8,
        len: u32,
        transferred: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn WinUsb_ControlTransfer(
        interface: Handle,
        setup: WinUsbSetupPacket,
        buffer: *mut u8,
        len: u32,
        transferred: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
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

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a `u16` string property of a device (its hardware ids, its service). `None` when the
/// property is absent.
fn property(set: Handle, info: &SpDevinfoData, property: u32) -> Option<Vec<u16>> {
    let mut kind = 0u32;
    let mut required = 0u32;
    // SAFETY: a null buffer asks only for the size; the call fails with it set.
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            info,
            property,
            &mut kind,
            null_mut(),
            0,
            &mut required,
        );
    }
    if required == 0 || last_error() != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buffer = vec![0u8; required as usize];
    // SAFETY: `buffer` holds `required` bytes, the size the call asked for.
    let ok = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            info,
            property,
            &mut kind,
            buffer.as_mut_ptr(),
            required,
            &mut required,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(
        buffer
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect(),
    )
}

fn units_to_string(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// The first `VID_xxxx`/`PID_xxxx` pair in a hardware id list.
fn vendor_product(hardware_ids: &[u16]) -> Option<(u16, u16)> {
    let text = String::from_utf16_lossy(hardware_ids).to_ascii_uppercase();
    let hex = |tag: &str| -> Option<u16> {
        let start = text.find(tag)? + tag.len();
        u16::from_str_radix(text.get(start..start + 4)?, 16).ok()
    };
    Some((hex("VID_")?, hex("PID_")?))
}

/// Parses `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}`. `None` for anything else.
fn parse_guid(text: &str) -> Option<Guid> {
    let inner = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let parts: Vec<&str> = inner.split('-').collect();
    if parts.len() != 5 || parts[3].len() != 4 || parts[4].len() != 12 {
        return None;
    }
    let mut data4 = [0u8; 8];
    let tail = format!("{}{}", parts[3], parts[4]);
    for (slot, pair) in data4.iter_mut().zip(tail.as_bytes().chunks(2)) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(Guid {
        data1: u32::from_str_radix(parts[0], 16).ok()?,
        data2: u16::from_str_radix(parts[1], 16).ok()?,
        data3: u16::from_str_radix(parts[2], 16).ok()?,
        data4,
    })
}

/// The interface class GUIDs a device registers, from its device key.
fn interface_guids(set: Handle, info: &SpDevinfoData) -> Vec<Guid> {
    // SAFETY: the set and the device record are valid for the call; the key is closed below.
    let key = unsafe { SetupDiOpenDevRegKey(set, info, DICS_FLAG_GLOBAL, 0, DIREG_DEV, KEY_READ) };
    if key == 0 || key == -1 {
        return Vec::new();
    }
    let name = wide("DeviceInterfaceGUIDs");
    let mut kind = 0u32;
    let mut size = 0u32;
    // SAFETY: a null buffer asks only for the size.
    unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            null_mut(),
            &mut kind,
            null_mut(),
            &mut size,
        );
    }
    let mut guids = Vec::new();
    if size > 0 {
        let mut buffer = vec![0u8; size as usize];
        // SAFETY: `buffer` holds `size` bytes.
        let ok = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                null_mut(),
                &mut kind,
                buffer.as_mut_ptr(),
                &mut size,
            )
        };
        if ok == 0 && kind == REG_MULTI_SZ {
            let units: Vec<u16> = buffer
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect();
            for entry in units.split(|&u| u == 0).filter(|e| !e.is_empty()) {
                if let Some(guid) = parse_guid(&String::from_utf16_lossy(entry)) {
                    guids.push(guid);
                }
            }
        }
    }
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    guids
}

/// One WinUSB interface path with the ids of its device.
struct Candidate {
    vendor: u16,
    product: u16,
    path: String,
}

/// Every interface path of a device whose driver is WinUSB.
fn candidates() -> Vec<Candidate> {
    // SAFETY: no class, enumerator or parent is given; every device present is enumerated.
    let set = unsafe {
        SetupDiGetClassDevsW(
            null_mut(),
            null_mut(),
            null_mut(),
            DIGCF_ALLCLASSES | DIGCF_PRESENT,
        )
    };
    if set == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut found = Vec::new();
    for index in 0u32.. {
        // SAFETY: the record is zeroed with its size set; the call fills it.
        let mut info = SpDevinfoData {
            cb_size: std::mem::size_of::<SpDevinfoData>() as u32,
            class_guid: Guid {
                data1: 0,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            dev_inst: 0,
            reserved: 0,
        };
        // SAFETY: `set` is the list from above and `info` is a valid out-record. Any failure
        // (the end of the list included) ends the walk.
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut info) } == 0 {
            break;
        }
        let service = property(set, &info, SPDRP_SERVICE).map(|u| units_to_string(&u));
        if !service.is_some_and(|s| s.eq_ignore_ascii_case("WinUSB")) {
            continue;
        }
        let Some((vendor, product)) =
            property(set, &info, SPDRP_HARDWAREID).and_then(|u| vendor_product(&u))
        else {
            continue;
        };
        for guid in interface_guids(set, &info) {
            let mut interface = SpDeviceInterfaceData {
                cb_size: std::mem::size_of::<SpDeviceInterfaceData>() as u32,
                interface_class_guid: guid,
                flags: 0,
                reserved: 0,
            };
            // SAFETY: the set and device record are valid; `interface` is a valid out-record.
            let device_info: *const c_void = (&info as *const SpDevinfoData).cast();
            // SAFETY: the set and device record are valid; `interface` is a valid out-record.
            if unsafe { SetupDiEnumDeviceInterfaces(set, device_info, &guid, 0, &mut interface) }
                == 0
            {
                continue;
            }
            let mut required = 0u32;
            // SAFETY: a null buffer asks only for the size.
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
            let size_field: u32 = if std::mem::size_of::<usize>() == 8 {
                8
            } else {
                6
            };
            detail[..4].copy_from_slice(&size_field.to_le_bytes());
            // SAFETY: `detail` holds `required` bytes, and its first field carries the size.
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
                .collect();
            found.push(Candidate {
                vendor,
                product,
                path: units_to_string(&units),
            });
        }
    }
    // SAFETY: `set` was created above and is destroyed once.
    unsafe { SetupDiDestroyDeviceInfoList(set) };
    found
}

/// The WinUSB-bound devices present, by vendor and product id, each listed once.
///
/// Support: Partial. Listing is built; it finds nothing unless a device is bound to WinUSB.
pub fn list() -> Vec<DeviceId> {
    let mut ids: Vec<DeviceId> = candidates()
        .into_iter()
        .map(|c| DeviceId::Usb {
            vendor: c.vendor,
            product: c.product,
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids.dedup();
    ids
}

/// An event for one overlapped transfer, closed when dropped.
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
        // SAFETY: created by Event::new and closed here only.
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

/// How an overlapped transfer ended.
enum Finished {
    Done,
    TimedOut,
}

/// Finishes a transfer that was started: returns `Done` when it completed (now or within `wait_ms`),
/// and `TimedOut` after cancelling it. `started` is the result of the call that started it.
fn finish(
    file: Handle,
    event: &Event,
    op: &mut Overlapped,
    transferred: &mut u32,
    started: i32,
    wait_ms: u32,
) -> Result<Finished, DeviceError> {
    if started != 0 {
        return Ok(Finished::Done);
    }
    let error = last_error();
    if error != ERROR_IO_PENDING {
        return Err(DeviceError::Io(format!("transfer failed (error {error})")));
    }
    // SAFETY: the event belongs to this transfer and stays open for the wait.
    if unsafe { WaitForSingleObject(event.0, wait_ms) } == WAIT_OBJECT_0 {
        // SAFETY: the transfer has signalled, so reading its result does not wait.
        return match unsafe { GetOverlappedResult(file, op, transferred, 0) } {
            0 => Err(DeviceError::Io(format!(
                "transfer failed (error {})",
                last_error()
            ))),
            _ => Ok(Finished::Done),
        };
    }
    // SAFETY: the transfer is pending on this handle. Cancel it and wait for the cancellation, so
    // its buffers are no longer in use when this function returns.
    unsafe {
        CancelIo(file);
        if GetOverlappedResult(file, op, transferred, 1) != 0 {
            return Ok(Finished::Done);
        }
    }
    Ok(Finished::TimedOut)
}

/// An open WinUSB interface. Dropping it frees the interface and closes the device.
///
/// Support: Partial. See the module documentation.
pub struct WinUsbDevice {
    id: DeviceId,
    file: Handle,
    first: Handle,
    interface: Handle,
    bulk_in: Option<u8>,
    bulk_out: Option<u8>,
}

// SAFETY: the handles are owned by this value and released once, in Drop. Every operation takes
// `&mut self`, so one thread at a time uses them.
unsafe impl Send for WinUsbDevice {}

impl Drop for WinUsbDevice {
    fn drop(&mut self) {
        // SAFETY: each handle is released once. The associated interface is freed before the
        // first interface it came from, and the file is closed last.
        unsafe {
            if self.interface != self.first && !self.interface.is_null() {
                WinUsb_Free(self.interface);
            }
            if !self.first.is_null() {
                WinUsb_Free(self.first);
            }
            if self.file != INVALID_HANDLE_VALUE {
                CloseHandle(self.file);
            }
        }
    }
}

/// Opens the first WinUSB interface for `id`, then the interface number `options.usb_interface`.
///
/// Support: Partial. Built, not verified on a device.
pub fn open(id: &DeviceId, options: &OpenOptions) -> Result<WinUsbDevice, DeviceError> {
    let DeviceId::Usb { vendor, product } = id else {
        return Err(DeviceError::Denied(format!("{id} is not a USB device")));
    };
    let path = candidates()
        .into_iter()
        .find(|c| c.vendor == *vendor && c.product == *product)
        .map(|c| c.path)
        .ok_or_else(|| DeviceError::Denied(format!("{id} is not present")))?;
    let name = wide(&path);
    // SAFETY: the name is NUL-terminated; the handle is overlapped.
    let file = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return Err(DeviceError::Denied(format!(
            "{id} cannot be opened (error {})",
            last_error()
        )));
    }
    let mut first: Handle = null_mut();
    // SAFETY: `file` is open; on failure `first` stays null and `file` is closed below.
    if unsafe { WinUsb_Initialize(file, &mut first) } == 0 {
        let error = last_error();
        // SAFETY: `file` came from CreateFileW above.
        unsafe { CloseHandle(file) };
        return Err(DeviceError::Io(format!(
            "WinUSB refused {id} (error {error})"
        )));
    }
    // The struct owns the handles from here, so Drop releases them on any early return.
    let mut device = WinUsbDevice {
        id: id.clone(),
        file,
        first,
        interface: first,
        bulk_in: None,
        bulk_out: None,
    };
    if options.usb_interface > 0 {
        let mut associated: Handle = null_mut();
        // SAFETY: `first` is the interface handle from WinUsb_Initialize; the index is the
        // associated interface's (0 is the first interface, which is already open).
        if unsafe {
            WinUsb_GetAssociatedInterface(first, options.usb_interface - 1, &mut associated)
        } == 0
        {
            return Err(DeviceError::Denied(format!(
                "{id} has no interface {}",
                options.usb_interface
            )));
        }
        device.interface = associated;
    }
    let mut descriptor = UsbInterfaceDescriptor::default();
    // SAFETY: `device.interface` is open, and the descriptor is a valid out-record.
    if unsafe { WinUsb_QueryInterfaceSettings(device.interface, 0, &mut descriptor) } == 0 {
        return Err(DeviceError::Io(format!(
            "cannot read the interface of {id}"
        )));
    }
    for index in 0..descriptor.num_endpoints {
        let mut pipe = WinUsbPipeInformation::default();
        // SAFETY: the interface is open and the pipe record is a valid out-record.
        if unsafe { WinUsb_QueryPipe(device.interface, 0, index, &mut pipe) } == 0 {
            continue;
        }
        if pipe.pipe_type != PIPE_TYPE_BULK {
            continue;
        }
        if pipe.pipe_id & 0x80 != 0 {
            device.bulk_in.get_or_insert(pipe.pipe_id);
        } else {
            device.bulk_out.get_or_insert(pipe.pipe_id);
        }
    }
    Ok(device)
}

impl Transport for WinUsbDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let pipe = self.bulk_out.ok_or(DeviceError::NotSupported(
            "the interface has no bulk OUT endpoint",
        ))?;
        let event = Event::new()?;
        let mut op = overlapped(&event);
        let mut written = 0u32;
        // SAFETY: `bytes` and `op` stay alive until the transfer is finished or cancelled.
        let started = unsafe {
            usb_write_pipe(
                self.interface,
                pipe,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                (&mut op as *mut Overlapped).cast(),
            )
        };
        match finish(
            self.file,
            &event,
            &mut op,
            &mut written,
            started,
            TRANSFER_WAIT_MS,
        )? {
            Finished::Done => Ok(()),
            Finished::TimedOut => Err(DeviceError::Busy),
        }
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        if max == 0 {
            return Ok(Vec::new());
        }
        let pipe = self.bulk_in.ok_or(DeviceError::NotSupported(
            "the interface has no bulk IN endpoint",
        ))?;
        let event = Event::new()?;
        let mut op = overlapped(&event);
        let mut buffer = vec![0u8; max];
        let mut got = 0u32;
        // SAFETY: `buffer` and `op` stay alive until the transfer is finished or cancelled.
        let started = unsafe {
            usb_read_pipe(
                self.interface,
                pipe,
                buffer.as_mut_ptr(),
                max as u32,
                &mut got,
                (&mut op as *mut Overlapped).cast(),
            )
        };
        // Nothing waiting is not an error for a reader: it returns empty after a short wait.
        match finish(self.file, &event, &mut op, &mut got, started, 0)? {
            Finished::Done => {
                buffer.truncate(got as usize);
                Ok(buffer)
            }
            Finished::TimedOut => Ok(Vec::new()),
        }
    }

    fn control(&mut self, request: &ControlRequest) -> Result<Vec<u8>, DeviceError> {
        let request_type = if request.direction_in {
            request.request_type | 0x80
        } else {
            request.request_type & !0x80
        };
        let length = if request.direction_in {
            request.max_in as usize
        } else {
            request.data.len()
        };
        let mut buffer = if request.direction_in {
            vec![0u8; length]
        } else {
            request.data.clone()
        };
        let setup = WinUsbSetupPacket {
            request_type,
            request: request.request,
            value: request.value,
            index: request.index,
            length: length as u16,
        };
        let event = Event::new()?;
        let mut op = overlapped(&event);
        let mut moved = 0u32;
        // SAFETY: `buffer` and `op` stay alive until the transfer is finished or cancelled.
        let started = unsafe {
            WinUsb_ControlTransfer(
                self.interface,
                setup,
                buffer.as_mut_ptr(),
                length as u32,
                &mut moved,
                (&mut op as *mut Overlapped).cast(),
            )
        };
        match finish(
            self.file,
            &event,
            &mut op,
            &mut moved,
            started,
            TRANSFER_WAIT_MS,
        )? {
            Finished::Done => {
                if request.direction_in {
                    buffer.truncate(moved as usize);
                    Ok(buffer)
                } else {
                    Ok(Vec::new())
                }
            }
            Finished::TimedOut => Err(DeviceError::Io("control transfer timed out".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_devices_are_usb_ids() {
        for id in list() {
            assert!(matches!(id, DeviceId::Usb { .. }), "{id}");
        }
    }

    #[test]
    fn an_absent_device_is_refused_not_faked() {
        let id = DeviceId::Usb {
            vendor: 0xffff,
            product: 0xfffe,
        };
        assert!(matches!(
            open(&id, &OpenOptions::default()),
            Err(DeviceError::Denied(_))
        ));
    }

    #[test]
    fn hardware_ids_give_vendor_and_product() {
        let ids: Vec<u16> = "USB\\VID_0403&PID_6001&REV_0600\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(vendor_product(&ids), Some((0x0403, 0x6001)));
        assert_eq!(vendor_product(&wide("ACPI\\PNP0501")), None);
    }

    #[test]
    fn interface_guids_parse_and_reject_other_text() {
        let guid = parse_guid("{a5dcbf10-6530-11d2-901f-00c04fb951ed}").expect("guid");
        assert_eq!(guid.data1, 0xa5dc_bf10);
        assert_eq!(guid.data4, [0x90, 0x1f, 0x00, 0xc0, 0x4f, 0xb9, 0x51, 0xed]);
        assert!(parse_guid("a5dcbf10-6530-11d2-901f-00c04fb951ed").is_none());
        assert!(parse_guid("{not-a-guid}").is_none());
    }
}
