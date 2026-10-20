// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Rust-implemented completion handlers (RFC 0001). Both WebView2 completion handlers
//! have the same shape, `Invoke(HRESULT, object*)`, so one implementation serves both.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::com::{Guid, IUnknownVtbl, E_NOINTERFACE, E_POINTER, HRESULT, IID_IUNKNOWN, S_OK};

type Callback = Box<dyn FnOnce(HRESULT, *mut c_void)>;

// Live callback objects on this thread (allocated and not yet freed). Used by leak tests.
#[cfg(test)]
thread_local! {
    static LIVE: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Records one callback object allocated (+1) or freed (-1).
#[cfg(test)]
pub(crate) fn track(delta: isize) {
    LIVE.with(|c| c.set(c.get() + delta));
}

/// Non-test builds do not count.
#[cfg(not(test))]
pub(crate) fn track(_delta: isize) {}

/// Callback objects alive on this thread.
#[cfg(test)]
pub(crate) fn live() -> isize {
    LIVE.with(|c| c.get())
}

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
    track(1);
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
        track(-1);
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::com::ComPtr;

    const IID_TEST: Guid = Guid {
        data1: 0x7a5c_1e10,
        data2: 0x1111,
        data3: 0x2222,
        data4: [1, 2, 3, 4, 5, 6, 7, 8],
    };
    const IID_OTHER: Guid = Guid {
        data1: 0x7a5c_1e11,
        data2: 0x1111,
        data3: 0x2222,
        data4: [1, 2, 3, 4, 5, 6, 7, 8],
    };

    /// Counts how many times the value holding it is dropped.
    struct DropCounter(Rc<Cell<u32>>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    unsafe fn add_ref(p: *mut c_void) -> u32 {
        let vt = *(p as *const *const HandlerVtbl);
        ((*vt).base.add_ref)(p)
    }

    unsafe fn release_ref(p: *mut c_void) -> u32 {
        let vt = *(p as *const *const HandlerVtbl);
        ((*vt).base.release)(p)
    }

    unsafe fn query(p: *mut c_void, iid: &Guid) -> (HRESULT, *mut c_void) {
        let vt = *(p as *const *const HandlerVtbl);
        let mut out = std::ptr::null_mut();
        let hr = ((*vt).base.query_interface)(p, iid, &mut out);
        (hr, out)
    }

    #[test]
    fn reference_counts_and_frees_once() {
        let before = live();
        let calls = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let (c, d) = (calls.clone(), DropCounter(drops.clone()));
        let h = new_completed_handler(IID_TEST, move |_, _| {
            let _keep = &d;
            c.set(c.get() + 1);
        });
        assert_eq!(live(), before + 1);
        // SAFETY: `h` is a live handler; each call matches the vtable layout.
        unsafe {
            assert_eq!(add_ref(h), 2);
            assert_eq!(release_ref(h), 1);
            assert_eq!(release_ref(h), 0, "the last release frees the object");
        }
        assert_eq!(live(), before, "object freed");
        assert_eq!(drops.get(), 1, "callback dropped exactly once");
        assert_eq!(calls.get(), 0, "callback never ran");
    }

    #[test]
    fn invoke_runs_the_callback_once_then_drops_it() {
        let before = live();
        let calls = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let (c, d) = (calls.clone(), DropCounter(drops.clone()));
        let h = new_completed_handler(IID_TEST, move |_, _| {
            let _keep = &d;
            c.set(c.get() + 1);
        });
        // SAFETY: `h` is live; invoke takes the same arguments the runtime would pass.
        unsafe {
            let vt = *(h as *const *const HandlerVtbl);
            ((*vt).invoke)(h, S_OK, std::ptr::null_mut());
            ((*vt).invoke)(h, S_OK, std::ptr::null_mut());
            release_ref(h);
        }
        assert_eq!(calls.get(), 1, "a completion callback runs once");
        assert_eq!(drops.get(), 1, "the callback is dropped after its run");
        assert_eq!(live(), before, "object freed");
    }

    #[test]
    fn query_interface_accepts_iunknown_and_its_own_iid_only() {
        let before = live();
        let h = new_completed_handler(IID_TEST, |_, _| {});
        // SAFETY: `h` is live; every successful QueryInterface adds a reference we release.
        unsafe {
            let (hr, out) = query(h, &IID_IUNKNOWN);
            assert_eq!(hr, S_OK);
            assert_eq!(out, h);
            release_ref(out);

            let (hr, out) = query(h, &IID_TEST);
            assert_eq!(hr, S_OK);
            release_ref(out);

            let (hr, out) = query(h, &IID_OTHER);
            assert_eq!(hr, E_NOINTERFACE);
            assert!(out.is_null());

            let vt = *(h as *const *const HandlerVtbl);
            let mut out: *mut c_void = std::ptr::null_mut();
            let hr = ((*vt).base.query_interface)(h, std::ptr::null(), &mut out);
            assert_eq!(hr, E_POINTER);

            release_ref(h);
        }
        assert_eq!(live(), before, "object freed");
    }

    #[test]
    fn com_ptr_holds_one_reference_and_releases_it() {
        let before = live();
        let drops = Rc::new(Cell::new(0));
        let d = DropCounter(drops.clone());
        let h = new_completed_handler(IID_TEST, move |_, _| {
            let _keep = &d;
        });
        // SAFETY: `h` is live. The owning pointer takes one reference and must release exactly that.
        unsafe {
            let owned = ComPtr::from_borrowed(h).expect("non-null");
            assert_eq!(add_ref(h), 3, "owned pointer added one reference");
            release_ref(h);
            drop(owned);
            assert_eq!(
                add_ref(h),
                2,
                "dropping the owned pointer released its reference"
            );
            release_ref(h);
            release_ref(h);
        }
        assert_eq!(live(), before, "object freed");
        assert_eq!(drops.get(), 1, "callback dropped exactly once");
    }
}
