// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Minimal COM plumbing (RFC 0001): GUIDs, IUnknown, an owning pointer and string helpers.

use std::ffi::c_void;

use windows_sys::Win32::System::Com::CoTaskMemFree;

pub type HRESULT = i32;
pub const S_OK: HRESULT = 0;
pub const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;
pub const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_IUNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46],
};

/// The first three slots of every COM vtable.
#[repr(C)]
pub struct IUnknownVtbl {
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// An owned reference to a COM object. Releases it on drop.
pub struct ComPtr(*mut c_void);

impl ComPtr {
    /// Takes over a reference that the callee handed to us (no AddRef).
    ///
    /// # Safety
    /// `p` must be null or a live COM interface pointer whose reference now belongs to us.
    pub unsafe fn from_owned(p: *mut c_void) -> Option<Self> {
        if p.is_null() {
            None
        } else {
            Some(ComPtr(p))
        }
    }

    /// Takes a new reference to an object we do not own (AddRef).
    ///
    /// # Safety
    /// `p` must be null or a live COM interface pointer.
    pub unsafe fn from_borrowed(p: *mut c_void) -> Option<Self> {
        if p.is_null() {
            return None;
        }
        let vt = *(p as *const *const IUnknownVtbl);
        ((*vt).add_ref)(p);
        Some(ComPtr(p))
    }

    /// Asks the object for another interface (`QueryInterface`). The result holds its own
    /// reference. Fails with `E_NOINTERFACE` when the object does not implement `iid`.
    pub fn query(&self, iid: &Guid) -> Result<ComPtr, HRESULT> {
        let vt: &IUnknownVtbl = self.vtbl();
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self.0` is a live object; `iid` and `out` are valid for the call.
        let hr = unsafe { (vt.query_interface)(self.0, iid, &mut out) };
        if hr < 0 {
            return Err(hr);
        }
        // SAFETY: on success the object gave us one reference, which we now own.
        unsafe { ComPtr::from_owned(out) }.ok_or(E_POINTER)
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.0
    }

    /// Typed view of the vtable. `V` must begin with [`IUnknownVtbl`].
    pub fn vtbl<V>(&self) -> &V {
        // SAFETY: a ComPtr always points at a live object whose first field is a vtable pointer.
        // The caller's `V` matches the interface this ComPtr was created for.
        unsafe { &**(self.0 as *const *const V) }
    }
}

impl Clone for ComPtr {
    fn clone(&self) -> Self {
        // SAFETY: `self.0` is a live interface pointer (the constructors reject null), and
        // AddRef gives the clone a reference of its own.
        unsafe { ComPtr::from_borrowed(self.0) }.expect("a live COM pointer is not null")
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        // SAFETY: we hold one reference, and release it exactly once here.
        unsafe {
            let vt = *(self.0 as *const *const IUnknownVtbl);
            ((*vt).release)(self.0);
        }
    }
}

/// NUL-terminated UTF-16, as WebView2 and Win32 expect.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Reads a string that COM allocated, then frees it with CoTaskMemFree.
///
/// Reads a NUL-terminated UTF-16 string that the caller does not own and must not free.
///
/// # Safety
/// `p` must be null or point at a NUL-terminated UTF-16 string that stays valid for the call.
pub unsafe fn read_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// # Safety
/// `p` must be null or a NUL-terminated UTF-16 string allocated with CoTaskMemAlloc.
pub unsafe fn take_co_string(p: *mut u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
    CoTaskMemFree(p as *const c_void);
    s
}

/// Drains the calling thread's message queue. A window created on the thread is fully set up
/// only after its creation messages are processed; UI Automation asked before that can fail with
/// E_FAIL. The UI Automation and native tree tests call this before they read their windows.
#[cfg(test)]
pub(crate) fn pump_pending_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    let mut msg = MSG {
        hwnd: std::ptr::null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
    };
    // SAFETY: `msg` is a valid, writable MSG; the queue is only read and dispatched here.
    unsafe {
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Calls `attempt` until it succeeds, or until about a second has passed, and returns the last
/// error then. Between attempts it pumps the thread's messages and waits 25 ms. UI Automation
/// reports a window's element a short time after the window exists, so a read made right after
/// creating it can fail with E_FAIL and succeed a moment later.
#[cfg(test)]
pub(crate) fn retry_until_ready<T, E>(mut attempt: impl FnMut() -> Result<T, E>) -> Result<T, E> {
    const TRIES: u32 = 40;
    let mut tries = 0;
    loop {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(error) if tries >= TRIES => return Err(error),
            Err(_) => {
                tries += 1;
                pump_pending_messages();
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
}
