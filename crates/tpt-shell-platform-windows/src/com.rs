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
