// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Win32 serial ports (spec §10.2, M7): `COM1` to `COM999`, opened with `CreateFileW` and set up
//! with `GetCommState` and `SetCommState`.
//!
//! The bindings are written by hand. Each declaration was checked against the windows-sys
//! bindings for `Win32_Devices_Communication` and `Win32_Storage_FileSystem` (its `DCB` layout,
//! `COMMTIMEOUTS` and the `kernel32` signatures), and the constants against winbase.h.
//!
//! Writes are synchronous and use a write timeout, so a port that stops taking data returns
//! [`DeviceError::Busy`] rather than blocking forever. Reads never wait.

use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use crate::id::DeviceId;
use crate::transport::{DeviceError, Transport};

type Handle = *mut c_void;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
const ERROR_FILE_NOT_FOUND: i32 = 2;
const NOPARITY: u8 = 0;
const ONESTOPBIT: u8 = 0;
const DTR_CONTROL_ENABLE: u32 = 1;
const RTS_CONTROL_ENABLE: u32 = 1;
const MAXDWORD: u32 = u32::MAX;
/// Write timeout in milliseconds for a constant, per call.
const WRITE_TIMEOUT_MS: u32 = 1000;
/// Bytes per ReadFile or WriteFile call.
const CHUNK: usize = 4096;

/// The `DCB` structure from winbase.h. Field order and sizes match the header. The flag bits
/// (`fBinary`, `fDtrControl` and so on) are packed into `flags`, least significant bit first.
#[repr(C)]
struct Dcb {
    dcb_length: u32,
    baud_rate: u32,
    flags: u32,
    reserved: u16,
    xon_lim: u16,
    xoff_lim: u16,
    byte_size: u8,
    parity: u8,
    stop_bits: u8,
    xon_char: i8,
    xoff_char: i8,
    error_char: i8,
    eof_char: i8,
    evt_char: i8,
    reserved1: u16,
}

/// The `COMMTIMEOUTS` structure from winbase.h.
#[repr(C)]
struct CommTimeouts {
    read_interval_timeout: u32,
    read_total_timeout_multiplier: u32,
    read_total_timeout_constant: u32,
    write_total_timeout_multiplier: u32,
    write_total_timeout_constant: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *const c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: Handle,
    ) -> Handle;
    fn GetCommState(file: Handle, dcb: *mut Dcb) -> i32;
    fn SetCommState(file: Handle, dcb: *const Dcb) -> i32;
    fn SetCommTimeouts(file: Handle, timeouts: *const CommTimeouts) -> i32;
    fn ReadFile(
        file: Handle,
        buffer: *mut u8,
        bytes_to_read: u32,
        bytes_read: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn WriteFile(
        file: Handle,
        buffer: *const u8,
        bytes_to_write: u32,
        bytes_written: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn CloseHandle(object: Handle) -> i32;
}

/// An open serial port. Dropping it closes the handle.
pub struct WinSerial {
    id: DeviceId,
    handle: Handle,
}

// SAFETY: the handle is owned by this value and closed exactly once, in Drop. Every operation
// takes `&mut self`, so only one thread uses the handle at a time, which is what a synchronous
// Win32 file handle needs.
unsafe impl Send for WinSerial {}

/// The Win32 device path for a port name such as `COM3`, as a NUL-terminated UTF-16 string.
/// Only `COM1` to `COM999` are accepted, so the name cannot be a path or a device namespace.
fn device_path(port: &str) -> Result<Vec<u16>, DeviceError> {
    let digits = port
        .get(..3)
        .filter(|p| p.eq_ignore_ascii_case("COM"))
        .map(|_| &port[3..])
        .unwrap_or("");
    let ok = (1..=3).contains(&digits.len())
        && digits.bytes().all(|b| b.is_ascii_digit())
        && !digits.starts_with('0');
    if !ok {
        return Err(DeviceError::Denied(format!(
            "`{port}` is not a COM port name (COM1 to COM999)"
        )));
    }
    Ok(format!(r"\\.\COM{digits}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect())
}

/// Opens `port` (for example `COM3`) at `baud`, 8 data bits, no parity, one stop bit.
pub fn open(id: &DeviceId, port: &str, baud: u32) -> Result<WinSerial, DeviceError> {
    let path = device_path(port)?;
    // SAFETY: `path` is a NUL-terminated UTF-16 string that lives for the call. No security
    // attributes or template handle are used, which the documentation allows.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let err = io::Error::last_os_error();
        return Err(if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) {
            DeviceError::Denied(format!("{port} is not present"))
        } else {
            DeviceError::Io(format!("cannot open {port}: {err}"))
        });
    }
    // From here, Drop closes the handle if setup fails.
    let port_handle = WinSerial {
        id: id.clone(),
        handle,
    };
    port_handle.configure(baud)?;
    Ok(port_handle)
}

impl WinSerial {
    fn configure(&self, baud: u32) -> Result<(), DeviceError> {
        let mut dcb = Dcb {
            dcb_length: std::mem::size_of::<Dcb>() as u32,
            baud_rate: 0,
            flags: 0,
            reserved: 0,
            xon_lim: 0,
            xoff_lim: 0,
            byte_size: 0,
            parity: 0,
            stop_bits: 0,
            xon_char: 0,
            xoff_char: 0,
            error_char: 0,
            eof_char: 0,
            evt_char: 0,
            reserved1: 0,
        };
        // SAFETY: `dcb` is a `DCB`-layout struct whose length is set, as GetCommState requires.
        if unsafe { GetCommState(self.handle, &mut dcb) } == 0 {
            return Err(io_error("read the port settings"));
        }
        dcb.baud_rate = baud;
        dcb.byte_size = 8;
        dcb.parity = NOPARITY;
        dcb.stop_bits = ONESTOPBIT;
        // fBinary is bit 0. fDtrControl is bits 4 and 5 and fRtsControl is bits 12 and 13.
        dcb.flags = 1 | (DTR_CONTROL_ENABLE << 4) | (RTS_CONTROL_ENABLE << 12);
        // SAFETY: `dcb` was filled in by GetCommState and changed only in documented fields.
        if unsafe { SetCommState(self.handle, &dcb) } == 0 {
            return Err(io_error("set the port settings"));
        }
        let timeouts = CommTimeouts {
            // A read returns at once with whatever has arrived, possibly nothing.
            read_interval_timeout: MAXDWORD,
            read_total_timeout_multiplier: 0,
            read_total_timeout_constant: 0,
            write_total_timeout_multiplier: 0,
            write_total_timeout_constant: WRITE_TIMEOUT_MS,
        };
        // SAFETY: `timeouts` is a valid COMMTIMEOUTS for the duration of the call.
        if unsafe { SetCommTimeouts(self.handle, &timeouts) } == 0 {
            return Err(io_error("set the port timeouts"));
        }
        Ok(())
    }
}

impl Drop for WinSerial {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW and is closed only here.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

impl Transport for WinSerial {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let mut sent = 0;
        while sent < bytes.len() {
            let chunk = &bytes[sent..];
            let n = chunk.len().min(CHUNK) as u32;
            let mut written: u32 = 0;
            // SAFETY: `chunk` is valid for `n` bytes. The overlapped pointer is null, so the
            // call is synchronous and `written` is set before it returns.
            let ok = unsafe { WriteFile(self.handle, chunk.as_ptr(), n, &mut written, null_mut()) };
            if ok == 0 {
                return Err(io_error("write to the port"));
            }
            if written == 0 {
                return if sent == 0 {
                    Err(DeviceError::Busy)
                } else {
                    Err(DeviceError::Io(format!(
                        "serial write timed out after {sent} of {} bytes",
                        bytes.len()
                    )))
                };
            }
            sent += written as usize;
        }
        Ok(())
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut buf = vec![0u8; max.min(CHUNK)];
        let mut read: u32 = 0;
        // SAFETY: `buf` is valid for `buf.len()` bytes. The overlapped pointer is null.
        let ok = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io_error("read from the port"));
        }
        buf.truncate(read as usize);
        Ok(buf)
    }
}

fn io_error(what: &str) -> DeviceError {
    DeviceError::Io(format!("cannot {what}: {}", io::Error::last_os_error()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_com_port_names_reach_the_driver() {
        assert!(device_path("COM1").is_ok());
        assert!(device_path("com12").is_ok());
        assert!(device_path("COM999").is_ok());
        for bad in [
            "",
            "COM",
            "COM0",
            "COM1000",
            "COM3x",
            r"\\.\COM3",
            "LPT1",
            "C:\\x",
            "COM3\\..\\x",
        ] {
            assert!(device_path(bad).is_err(), "accepted `{bad}`");
        }
    }

    #[test]
    fn a_port_that_is_not_present_is_reported() {
        // COM250 is not a port on any machine this runs on, so the open must fail.
        let id: DeviceId = "serial:COM250".parse().expect("id");
        assert!(open(&id, "COM250", 9600).is_err());
    }

    #[test]
    fn a_named_port_opens_when_the_tester_gives_one() {
        let Ok(port) = std::env::var("TPT_TEST_SERIAL_PORT") else {
            eprintln!("skipped: set TPT_TEST_SERIAL_PORT to a free COM port to run this test");
            return;
        };
        let id: DeviceId = format!("serial:{port}").parse().expect("id");
        let mut device = open(&id, &port, 9600).expect("open the named port");
        device.write(&[]).expect("an empty write succeeds");
        assert!(device.read(16).expect("read").len() <= 16);
    }
}
