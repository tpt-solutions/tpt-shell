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
    crate::handler::track(1);
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
        crate::handler::track(-1);
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    const IID_TEST: Guid = Guid {
        data1: 0x7a5c_2e10,
        data2: 0x1111,
        data3: 0x2222,
        data4: [1, 2, 3, 4, 5, 6, 7, 8],
    };

    struct DropCounter(Rc<Cell<u32>>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn event_callback_runs_each_time_and_is_dropped_on_free() {
        let before = crate::handler::live();
        let calls = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let (c, d) = (calls.clone(), DropCounter(drops.clone()));
        let h = new_event_handler(IID_TEST, move |_, _| {
            let _keep = &d;
            c.set(c.get() + 1);
        });
        // SAFETY: `h` is live; the vtable is the one this module built for it.
        unsafe {
            let vt = *(h as *const *const EventVtbl);
            ((*vt).invoke)(h, std::ptr::null_mut(), std::ptr::null_mut());
            ((*vt).invoke)(h, std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(calls.get(), 2, "an event callback runs every time it fires");
            assert_eq!(
                drops.get(),
                0,
                "the callback lives until the object is freed"
            );
            ((*vt).base.release)(h);
        }
        assert_eq!(drops.get(), 1, "callback dropped once, on free");
        assert_eq!(crate::handler::live(), before, "object freed");
    }
}
