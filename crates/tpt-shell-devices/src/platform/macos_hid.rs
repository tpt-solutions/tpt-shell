// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! HID devices on macOS through IOKit's `IOHIDManager` (spec §10.2, M7).
//!
//! The declarations are written by hand from `IOHIDManager.h`, `IOHIDDevice.h` and the
//! CoreFoundation headers. All devices are enumerated (no matching dictionary) and filtered by
//! the `VendorID` and `ProductID` properties.
//!
//! Input reports arrive through a callback that runs on a run loop. Each `read` schedules the
//! device on the calling thread's run loop, runs it once without waiting, and unschedules it, so
//! the transport can move between threads. Reports that arrive between reads are queued by the
//! driver, which may drop old ones if reads are rare. A `write` sends one output report whose
//! first byte is the report ID (0 when the device uses no numbered reports, in which case that
//! byte is not sent), as the HID utilities do.
//!
//! Opening a keyboard-class device needs the Input Monitoring permission; without it the open is
//! denied with a message that says so.
//!
//! Support: Partial. Compile-checked from a Windows host; never run on a Mac from this
//! workspace. The runtime test runs only when a HID device is present and may be opened.

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::sync::Mutex;

use crate::id::DeviceId;
use crate::transport::{DeviceError, Transport};

type CFTypeRef = *const c_void;
type CFIndex = isize;
type IOReturn = i32;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_CF_NUMBER_SINT32_TYPE: CFIndex = 3;
const K_IO_HID_REPORT_TYPE_OUTPUT: i32 = 1;
const K_IO_RETURN_SUCCESS: IOReturn = 0;
const K_IO_RETURN_EXCLUSIVE_ACCESS: IOReturn = 0xe000_02c5_u32 as i32;
const K_IO_RETURN_NOT_PERMITTED: IOReturn = 0xe000_02e2_u32 as i32;
/// Largest input report the transport accepts.
const MAX_REPORT: usize = 4096;
/// Most reports queued between reads.
const MAX_QUEUED: usize = 256;

type InputCallback = extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    report_type: i32,
    report_id: u32,
    report: *mut u8,
    report_length: CFIndex,
);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: CFTypeRef;
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        c_str: *const c_char,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFRelease(object: CFTypeRef);
    fn CFRetain(object: CFTypeRef) -> CFTypeRef;
    fn CFSetGetCount(set: CFTypeRef) -> CFIndex;
    fn CFSetGetValues(set: CFTypeRef, values: *mut CFTypeRef);
    fn CFNumberGetValue(number: CFTypeRef, the_type: CFIndex, value: *mut c_void) -> u8;
    fn CFRunLoopGetCurrent() -> CFTypeRef;
    fn CFRunLoopRunInMode(mode: CFTypeRef, seconds: f64, return_after_source_handled: u8) -> i32;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: CFTypeRef, options: u32) -> CFTypeRef;
    fn IOHIDManagerSetDeviceMatching(manager: CFTypeRef, matching: CFTypeRef);
    fn IOHIDManagerOpen(manager: CFTypeRef, options: u32) -> IOReturn;
    fn IOHIDManagerClose(manager: CFTypeRef, options: u32) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: CFTypeRef) -> CFTypeRef;
    fn IOHIDDeviceGetProperty(device: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn IOHIDDeviceOpen(device: CFTypeRef, options: u32) -> IOReturn;
    fn IOHIDDeviceClose(device: CFTypeRef, options: u32) -> IOReturn;
    fn IOHIDDeviceSetReport(
        device: CFTypeRef,
        report_type: i32,
        report_id: CFIndex,
        report: *const u8,
        report_length: CFIndex,
    ) -> IOReturn;
    fn IOHIDDeviceRegisterInputReportCallback(
        device: CFTypeRef,
        report: *mut u8,
        report_length: CFIndex,
        callback: Option<InputCallback>,
        context: *mut c_void,
    );
    fn IOHIDDeviceScheduleWithRunLoop(device: CFTypeRef, run_loop: CFTypeRef, mode: CFTypeRef);
    fn IOHIDDeviceUnscheduleFromRunLoop(device: CFTypeRef, run_loop: CFTypeRef, mode: CFTypeRef);
}

/// A CoreFoundation object released when dropped.
struct Owned(CFTypeRef);

impl Owned {
    fn new(object: CFTypeRef) -> Option<Self> {
        (!object.is_null()).then_some(Owned(object))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a CoreFoundation Create or Copy call (or CFRetain), so
        // this value owns one reference, which is released exactly once here.
        unsafe { CFRelease(self.0) }
    }
}

/// An opened `IOHIDManager`. Dropping it closes the manager, then releases it.
struct Manager(Owned);

impl Drop for Manager {
    fn drop(&mut self) {
        // SAFETY: the manager was opened in `Enumeration::new` and is still live: the `Owned`
        // field is released after this runs.
        unsafe {
            IOHIDManagerClose(self.0 .0, 0);
        }
    }
}

/// An opened manager and every HID device it sees.
struct Enumeration {
    manager: Manager,
    devices: Vec<Owned>,
}

impl Enumeration {
    fn new() -> Option<Self> {
        // SAFETY: a null allocator selects the default one. A null matching dictionary matches
        // all devices.
        let manager = Owned::new(unsafe { IOHIDManagerCreate(std::ptr::null(), 0) })?;
        // SAFETY: `manager` is a live IOHIDManager.
        unsafe {
            IOHIDManagerSetDeviceMatching(manager.0, std::ptr::null());
            IOHIDManagerOpen(manager.0, 0);
        }
        // SAFETY: `manager` is live. The returned set (a Copy) is owned and released below.
        let set = Owned::new(unsafe { IOHIDManagerCopyDevices(manager.0) })?;
        // SAFETY: `set` is a live CFSet.
        let count = unsafe { CFSetGetCount(set.0) }.max(0) as usize;
        let mut values: Vec<CFTypeRef> = vec![std::ptr::null(); count];
        if count > 0 {
            // SAFETY: `values` has room for `count` pointers, the size of the set.
            unsafe { CFSetGetValues(set.0, values.as_mut_ptr()) };
        }
        let devices = values
            .into_iter()
            .filter(|d| !d.is_null())
            // SAFETY: each value is a live IOHIDDevice owned by the set. The retain gives this
            // value its own reference, so it stays valid after `set` is released.
            .filter_map(|d| Owned::new(unsafe { CFRetain(d) }))
            .collect();
        Some(Enumeration {
            manager: Manager(manager),
            devices,
        })
    }
}

fn int_property(device: CFTypeRef, name: &str) -> Option<i32> {
    let key = CString::new(name).ok()?;
    // SAFETY: `key` is a NUL-terminated string that lives for the call.
    let key = Owned::new(unsafe {
        CFStringCreateWithCString(std::ptr::null(), key.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    })?;
    // SAFETY: `device` is a live IOHIDDevice and `key` a live CFString. The result is not owned
    // (a Get), so it is not released.
    let value = unsafe { IOHIDDeviceGetProperty(device, key.0) };
    if value.is_null() {
        return None;
    }
    let mut out: i32 = 0;
    // SAFETY: `value` is a property object; CFNumberGetValue returns 0 when it is not a number
    // that fits, and writes at most 4 bytes to `out`.
    let ok = unsafe {
        CFNumberGetValue(
            value,
            K_CF_NUMBER_SINT32_TYPE,
            (&mut out as *mut i32).cast::<c_void>(),
        )
    };
    (ok != 0).then_some(out)
}

fn vendor_product(device: CFTypeRef) -> Option<(u16, u16)> {
    let vendor = u16::try_from(int_property(device, "VendorID")?).ok()?;
    let product = u16::try_from(int_property(device, "ProductID")?).ok()?;
    Some((vendor, product))
}

/// The HID devices present, once per vendor and product.
///
/// Support: Partial. Compile-checked; never run on a Mac from this workspace.
pub fn list() -> Vec<DeviceId> {
    let Some(found) = Enumeration::new() else {
        return Vec::new();
    };
    let mut ids: Vec<DeviceId> = Vec::new();
    for device in &found.devices {
        if let Some((vendor, product)) = vendor_product(device.0) {
            let id = DeviceId::Hid { vendor, product };
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

type Queue = Mutex<VecDeque<Vec<u8>>>;

extern "C" fn on_report(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _report_type: i32,
    _report_id: u32,
    report: *mut u8,
    report_length: CFIndex,
) {
    if context.is_null() || report.is_null() || result != K_IO_RETURN_SUCCESS || report_length <= 0
    {
        return;
    }
    // SAFETY: `context` is the Queue registered by `open`, kept alive until the callback is
    // unregistered in Drop. IOKit passes a report valid for `report_length` bytes for the call.
    let (queue, data) = unsafe {
        (
            &*(context as *const Queue),
            std::slice::from_raw_parts(report, report_length as usize),
        )
    };
    let mut queue = queue.lock().unwrap_or_else(|p| p.into_inner());
    if queue.len() >= MAX_QUEUED {
        queue.pop_front();
    }
    queue.push_back(data.to_vec());
}

/// An open HID device. Dropping it unregisters the callback and closes the device.
///
/// Support: Partial. Compile-checked; never run on a Mac from this workspace.
pub struct IoKitHid {
    id: DeviceId,
    device: Owned,
    _manager: Manager,
    /// The buffer IOKit writes each input report into. Must outlive the registration.
    _buffer: Box<[u8]>,
    queue: *mut Queue,
}

// SAFETY: the CoreFoundation objects are reference-counted and thread-safe to retain and release.
// Every IOKit call on the device is made from `&mut self` methods, so one thread uses it at a
// time, and the run loop is chosen per call (the thread that calls `read`). The queue pointer is
// owned by this value and shared with the callback only through its Mutex.
unsafe impl Send for IoKitHid {}

/// Opens the first HID device whose vendor and product match `id`.
///
/// Support: Partial. Compile-checked; never run on a Mac from this workspace.
pub fn open(id: &DeviceId) -> Result<IoKitHid, DeviceError> {
    let DeviceId::Hid { vendor, product } = id else {
        return Err(DeviceError::Denied(format!("{id} is not a HID device")));
    };
    let found = Enumeration::new()
        .ok_or_else(|| DeviceError::Io("cannot enumerate HID devices".to_string()))?;
    let Some(index) = found
        .devices
        .iter()
        .position(|d| vendor_product(d.0) == Some((*vendor, *product)))
    else {
        return Err(DeviceError::Denied(format!("{id} is not present")));
    };
    let Enumeration {
        manager,
        mut devices,
    } = found;
    // The manager stays open for as long as the device is open.
    let device = devices.swap_remove(index);
    // SAFETY: `device` is a live IOHIDDevice.
    let status = unsafe { IOHIDDeviceOpen(device.0, 0) };
    match status {
        K_IO_RETURN_SUCCESS => {}
        K_IO_RETURN_NOT_PERMITTED => {
            return Err(DeviceError::Denied(format!(
                "{id} needs the Input Monitoring permission (System Settings, Privacy and Security)"
            )))
        }
        K_IO_RETURN_EXCLUSIVE_ACCESS => {
            return Err(DeviceError::Denied(format!(
                "{id} is open exclusively by another process"
            )))
        }
        other => {
            return Err(DeviceError::Io(format!(
                "cannot open {id}: IOKit status {other:#x}"
            )))
        }
    }
    let mut buffer = vec![0u8; MAX_REPORT].into_boxed_slice();
    let queue = Box::into_raw(Box::new(Mutex::new(VecDeque::new())));
    // SAFETY: the buffer and the queue outlive the registration, which Drop removes before it
    // frees them. The callback has the signature IOKit expects.
    unsafe {
        IOHIDDeviceRegisterInputReportCallback(
            device.0,
            buffer.as_mut_ptr(),
            MAX_REPORT as CFIndex,
            Some(on_report),
            queue.cast(),
        );
    }
    Ok(IoKitHid {
        id: id.clone(),
        device,
        _manager: manager,
        _buffer: buffer,
        queue,
    })
}

impl Drop for IoKitHid {
    fn drop(&mut self) {
        // SAFETY: the device is live. Unregistering the callback first means nothing can use the
        // queue or the buffer afterwards, so the queue (made with Box::into_raw in `open`) is
        // freed exactly once here.
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(
                self.device.0,
                self._buffer.as_mut_ptr(),
                0,
                None,
                std::ptr::null_mut(),
            );
            IOHIDDeviceClose(self.device.0, 0);
            drop(Box::from_raw(self.queue));
        }
    }
}

impl Transport for IoKitHid {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let Some((&report_id, rest)) = bytes.split_first() else {
            return Ok(());
        };
        // Report 0 is "no numbered reports": the ID byte is not part of what is sent.
        let payload = if report_id == 0 { rest } else { bytes };
        // SAFETY: the device is open, and `payload` is valid for its length for the call.
        let status = unsafe {
            IOHIDDeviceSetReport(
                self.device.0,
                K_IO_HID_REPORT_TYPE_OUTPUT,
                CFIndex::from(report_id),
                payload.as_ptr(),
                payload.len() as CFIndex,
            )
        };
        if status == K_IO_RETURN_SUCCESS {
            Ok(())
        } else {
            Err(DeviceError::Io(format!(
                "cannot send the report: IOKit status {status:#x}"
            )))
        }
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        // SAFETY: the device is open. The run loop and mode are the calling thread's and the
        // process-wide default mode; the device is unscheduled again before returning, so no
        // reference to this thread's run loop is kept.
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            IOHIDDeviceScheduleWithRunLoop(self.device.0, run_loop, kCFRunLoopDefaultMode);
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.0, 1);
            IOHIDDeviceUnscheduleFromRunLoop(self.device.0, run_loop, kCFRunLoopDefaultMode);
        }
        // SAFETY: the queue was made in `open` and lives until Drop.
        let queue = unsafe { &*self.queue };
        let mut queue = queue.lock().unwrap_or_else(|p| p.into_inner());
        let mut report = queue.pop_front().unwrap_or_default();
        report.truncate(max);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_that_is_absent_is_denied() {
        let id: DeviceId = "hid:ffff:fffe".parse().expect("id");
        assert!(matches!(open(&id), Err(DeviceError::Denied(_))));
    }

    #[test]
    fn a_present_device_reads_without_blocking() {
        let Some(id) = list().into_iter().next() else {
            eprintln!("skipped: no HID device is present on this machine");
            return;
        };
        match open(&id) {
            Ok(mut device) => {
                let report = device.read(64).expect("a read never blocks");
                assert!(report.len() <= 64);
            }
            Err(DeviceError::Denied(why)) => eprintln!("skipped: {why}"),
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
}
