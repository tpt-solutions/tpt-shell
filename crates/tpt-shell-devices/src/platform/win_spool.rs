// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Raw printing through the Windows print spooler (spec §10.2, M7). The bytes go to the printer
//! unchanged, as a RAW document, so ESC/POS and ZPL reach the printer intact.
//!
//! The winspool bindings are written by hand. Each declaration was checked against the
//! windows-sys bindings for `Win32_Graphics_Printing` (the `winspool.drv` signatures and the
//! `DOC_INFO_1W` layout).

use std::ffi::c_void;
use std::io;
use std::ptr::{null, null_mut};

use crate::transport::DeviceError;

type Handle = *mut c_void;

/// `DOC_INFO_1W` from winspool.h. Level 1 of `StartDocPrinterW`.
#[repr(C)]
struct DocInfo1W {
    doc_name: *const u16,
    output_file: *const u16,
    datatype: *const u16,
}

/// Most bytes passed to one WritePrinter call.
const CHUNK: usize = 64 * 1024;

#[link(name = "winspool")]
extern "system" {
    fn OpenPrinterW(printer_name: *const u16, printer: *mut Handle, defaults: *const c_void)
        -> i32;
    fn ClosePrinter(printer: Handle) -> i32;
    fn StartDocPrinterW(printer: Handle, level: u32, doc_info: *const DocInfo1W) -> u32;
    fn EndDocPrinter(printer: Handle) -> i32;
    fn StartPagePrinter(printer: Handle) -> i32;
    fn EndPagePrinter(printer: Handle) -> i32;
    fn WritePrinter(
        printer: Handle,
        buffer: *const c_void,
        bytes_to_write: u32,
        bytes_written: *mut u32,
    ) -> i32;
    fn GetDefaultPrinterW(buffer: *mut u16, buffer_chars: *mut u32) -> i32;
}

/// An open printer handle. Dropping it closes the handle.
struct OpenPrinter(Handle);

impl Drop for OpenPrinter {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful OpenPrinterW and is closed only here.
        unsafe {
            ClosePrinter(self.0);
        }
    }
}

/// NUL-terminated UTF-16 for a Win32 string argument.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn io_error(what: &str) -> DeviceError {
    DeviceError::Io(format!("cannot {what}: {}", io::Error::last_os_error()))
}

fn open_printer(name: &str) -> Result<OpenPrinter, DeviceError> {
    let name = wide(name);
    let mut handle: Handle = null_mut();
    // SAFETY: `name` is NUL-terminated and lives for the call. `defaults` may be null, and the
    // handle is written only on success.
    let ok = unsafe { OpenPrinterW(name.as_ptr(), &mut handle, null()) };
    if ok == 0 {
        return Err(io_error("open the printer"));
    }
    Ok(OpenPrinter(handle))
}

/// The name of the default printer, if Windows has one.
///
/// Support: Full.
pub fn default_printer() -> Option<String> {
    let mut chars: u32 = 0;
    // SAFETY: a null buffer with a size pointer asks for the length needed.
    unsafe { GetDefaultPrinterW(null_mut(), &mut chars) };
    if chars == 0 {
        return None;
    }
    let mut buf = vec![0u16; chars as usize];
    // SAFETY: `buf` holds `chars` UTF-16 units, which is the size the call reported.
    let ok = unsafe { GetDefaultPrinterW(buf.as_mut_ptr(), &mut chars) };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Sends `bytes` to `printer` as one RAW document named `job`.
///
/// Support: Full.
pub fn send_raw(printer: &str, job: &str, bytes: &[u8]) -> Result<(), DeviceError> {
    let handle = open_printer(printer)?;
    let job = wide(job);
    let raw = wide("RAW");
    let info = DocInfo1W {
        doc_name: job.as_ptr(),
        output_file: null(),
        datatype: raw.as_ptr(),
    };
    // SAFETY: `info` and the strings it points to live until the call returns.
    if unsafe { StartDocPrinterW(handle.0, 1, &info) } == 0 {
        return Err(io_error("start the print job"));
    }
    let written = write_page(handle.0, bytes);
    // End the document whatever happened to the page. A failed write is the error reported.
    // SAFETY: the document was started above, and this ends it once.
    let ended = unsafe { EndDocPrinter(handle.0) };
    written?;
    if ended == 0 {
        return Err(io_error("end the print job"));
    }
    Ok(())
}

fn write_page(handle: Handle, bytes: &[u8]) -> Result<(), DeviceError> {
    // SAFETY: the document is open; the page is ended below on success.
    if unsafe { StartPagePrinter(handle) } == 0 {
        return Err(io_error("start the page"));
    }
    let mut sent = 0;
    while sent < bytes.len() {
        let chunk = &bytes[sent..];
        let n = chunk.len().min(CHUNK) as u32;
        let mut written: u32 = 0;
        // SAFETY: `chunk` is valid for `n` bytes and `written` is a valid out pointer.
        let ok = unsafe { WritePrinter(handle, chunk.as_ptr().cast::<c_void>(), n, &mut written) };
        if ok == 0 || written == 0 {
            return Err(io_error("write to the printer"));
        }
        sent += written as usize;
    }
    // SAFETY: the page was started above.
    if unsafe { EndPagePrinter(handle) } == 0 {
        return Err(io_error("end the page"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printer_that_does_not_exist_cannot_be_opened() {
        assert!(open_printer("TPT-no-such-printer-7f3a91").is_err());
    }

    #[test]
    fn the_default_printer_opens_when_there_is_one() {
        let Some(name) = default_printer() else {
            eprintln!("skipped: this machine has no default printer");
            return;
        };
        open_printer(&name).expect("the default printer opens");
    }
}
