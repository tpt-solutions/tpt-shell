// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Rust-implemented completion handlers (RFC 0001). Both WebView2 completion handlers
//! have the same shape, `Invoke(HRESULT, object*)`, so one implementation serves both.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::com::{Guid, IUnknownVtbl, E_NOINTERFACE, E_POINTER, HRESULT, IID_IUNKNOWN, S_OK};

type Callback = Box<dyn FnOnce(HRESULT, *mut c_void)>;

#[repr(C)]
struct HandlerVtbl {
    base: IUnknownVtbl,
    invoke: unsafe extern "system" fn(*mut c_void, HRESULT, *mut c_void) -> HRESULT,
}

/// Layout starts with the vtable pointer, as COM requires.
#[repr(C)]
struct CompletedHandler {
    vtbl: *const HandlerVtbl,
    refs: AtomicU32,
    iid: Guid,
    callback: Cell<Option<Callback>>,
}

static VTBL: HandlerVtbl = HandlerVtbl {
    base: IUnknownVtbl {
        query_interface,
        add_ref,
        release: release_impl,
    },
    invoke,
};

/// Creates a handler with one reference, owned by the caller. `iid` is the handler
/// interface, which QueryInterface must accept. `callback` runs once, on the UI thread.
pub fn new_completed_handler(
    iid: Guid,
    callback: impl FnOnce(HRESULT, *mut c_void) + 'static,
) -> *mut c_void {
    let boxed = Box::new(CompletedHandler {
        vtbl: &VTBL,
        refs: AtomicU32::new(1),
        iid,
        callback: Cell::new(Some(Box::new(callback))),
    });
    Box::into_raw(boxed) as *mut c_void
}

/// Drops one reference the caller holds on a handler.
///
/// # Safety
/// `p` must come from [`new_completed_handler`] and the caller must not use it afterwards
/// unless another reference was taken.
pub unsafe fn release(p: *mut c_void) {
    let vt = *(p as *const *const HandlerVtbl);
    ((*vt).base.release)(p);
}

unsafe extern "system" fn query_interface(
    this: *mut c_void,
    riid: *const Guid,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    let h = this as *const CompletedHandler;
    let wanted = *riid;
    if wanted == IID_IUNKNOWN || wanted == (*h).iid {
        *ppv = this;
        add_ref(this);
        S_OK
    } else {
        *ppv = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    (*(this as *const CompletedHandler))
        .refs
        .fetch_add(1, Ordering::AcqRel)
        + 1
}

unsafe extern "system" fn release_impl(this: *mut c_void) -> u32 {
    let remaining = (*(this as *const CompletedHandler))
        .refs
        .fetch_sub(1, Ordering::AcqRel)
        - 1;
    if remaining == 0 {
        drop(Box::from_raw(this as *mut CompletedHandler));
    }
    remaining
}

unsafe extern "system" fn invoke(this: *mut c_void, hr: HRESULT, obj: *mut c_void) -> HRESULT {
    let h = this as *const CompletedHandler;
    if let Some(cb) = (*h).callback.take() {
        cb(hr, obj);
    }
    S_OK
}
