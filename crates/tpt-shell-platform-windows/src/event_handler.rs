// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Rust-implemented event handlers (RFC 0001). Unlike completion handlers, these run
//! many times, so the callback is `FnMut`. Their `Invoke` takes `(sender, args)`.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::com::{Guid, IUnknownVtbl, E_NOINTERFACE, E_POINTER, HRESULT, IID_IUNKNOWN, S_OK};

type Callback = Box<dyn FnMut(*mut c_void, *mut c_void)>;

#[repr(C)]
struct EventVtbl {
    base: IUnknownVtbl,
    invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

/// Starts with the vtable pointer, as COM requires. Same layout idea as `handler::CompletedHandler`.
#[repr(C)]
struct EventHandler {
    vtbl: *const EventVtbl,
    refs: AtomicU32,
    iid: Guid,
    callback: RefCell<Callback>,
}

static VTBL: EventVtbl = EventVtbl {
    base: IUnknownVtbl {
        query_interface,
        add_ref,
        release: release_impl,
    },
    invoke,
};

/// Creates an event handler with one reference, owned by the caller. `iid` is the handler
/// interface, which QueryInterface must accept. `callback` runs on the UI thread each time
/// the event fires, with the raw sender and args pointers, which are valid only during the call.
pub fn new_event_handler(
    iid: Guid,
    callback: impl FnMut(*mut c_void, *mut c_void) + 'static,
) -> *mut c_void {
    let boxed = Box::new(EventHandler {
        vtbl: &VTBL,
        refs: AtomicU32::new(1),
        iid,
        callback: RefCell::new(Box::new(callback)),
    });
    Box::into_raw(boxed) as *mut c_void
}

unsafe extern "system" fn query_interface(
    this: *mut c_void,
    riid: *const Guid,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    let h = this as *const EventHandler;
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
    (*(this as *const EventHandler))
        .refs
        .fetch_add(1, Ordering::AcqRel)
        + 1
}

unsafe extern "system" fn release_impl(this: *mut c_void) -> u32 {
    let remaining = (*(this as *const EventHandler))
        .refs
        .fetch_sub(1, Ordering::AcqRel)
        - 1;
    if remaining == 0 {
        drop(Box::from_raw(this as *mut EventHandler));
    }
    remaining
}

unsafe extern "system" fn invoke(
    this: *mut c_void,
    sender: *mut c_void,
    args: *mut c_void,
) -> HRESULT {
    let h = this as *const EventHandler;
    (*(*h).callback.borrow_mut())(sender, args);
    S_OK
}
