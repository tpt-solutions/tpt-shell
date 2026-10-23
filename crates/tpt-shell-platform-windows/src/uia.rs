// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The native accessibility tree of a window, read through UI Automation (spec §10.6).
//!
//! [`dump`] returns one [`UiaNode`] for each element under a window, the window's own element
//! included, in the order UI Automation returns them. A node has the role (named as the ARIA
//! role the DOM side uses, so the two can be compared), the name, whether it is enabled, and its
//! bounding rectangle in screen pixels.
//!
//! The COM interfaces are called by hand, as the WebView2 layer is (see `webview2.rs`). Each call
//! reads its function from the object's vtable at a fixed slot index. The indices are the ones in
//! `UIAutomationClient.h`, and the header test at the end of this file checks every index against
//! the header, so a wrong index fails the tests rather than calling the wrong method.
//!
//! Support: Partial. Windows only, through UI Automation's view of the window's subtree. An
//! element with no bounding rectangle reports no bounds. The dump is not yet wired to the test
//! driver's `NativeTree` trait: that trait lives in tpt-shell-test, which this crate does not
//! depend on, so the conversion happens where the comparison is wired.

use std::ffi::c_void;
use std::ptr::null_mut;

use tpt_shell_core::Error;
use windows_sys::Win32::Foundation::{SysFreeString, HWND, RECT};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};

use crate::com::{read_wide, ComPtr, Guid, HRESULT};

/// `CUIAutomation`, the class that creates the UI Automation root (`UIAutomationClient.h`).
///
/// Support: Full.
pub const CLSID_CUI_AUTOMATION: Guid = Guid {
    data1: 0xff48_dba4,
    data2: 0x60ef,
    data3: 0x4201,
    data4: [0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e],
};

/// `IUIAutomation` (`UIAutomationClient.h`).
///
/// Support: Full.
pub const IID_IUI_AUTOMATION: Guid = Guid {
    data1: 0x30cb_e57d,
    data2: 0xd9d0,
    data3: 0x452a,
    data4: [0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee],
};

/// `TreeScope_Subtree`: the element, its children and all their descendants.
const TREE_SCOPE_SUBTREE: i32 = 0x1 | 0x2 | 0x4;

// Vtable slot indices, from UIAutomationClient.h. Each is checked by `used_slots_match_the_uia_header`.

/// `IUIAutomation::ElementFromHandle`.
const SLOT_ELEMENT_FROM_HANDLE: usize = 6;
/// `IUIAutomation::CreateTrueCondition`.
const SLOT_CREATE_TRUE_CONDITION: usize = 21;
/// `IUIAutomationElement::FindAll`.
const SLOT_FIND_ALL: usize = 6;
/// `IUIAutomationElement::get_CurrentControlType`.
const SLOT_GET_CURRENT_CONTROL_TYPE: usize = 21;
/// `IUIAutomationElement::get_CurrentName`.
const SLOT_GET_CURRENT_NAME: usize = 23;
/// `IUIAutomationElement::get_CurrentIsEnabled`.
const SLOT_GET_CURRENT_IS_ENABLED: usize = 28;
/// `IUIAutomationElement::get_CurrentBoundingRectangle`.
const SLOT_GET_CURRENT_BOUNDING_RECTANGLE: usize = 43;
/// `IUIAutomationElementArray::get_Length`.
const SLOT_GET_LENGTH: usize = 3;
/// `IUIAutomationElementArray::GetElement`.
const SLOT_GET_ELEMENT: usize = 4;

/// Signatures of the calls this module makes, as the header declares them.
type ElementFromHandleFn =
    unsafe extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT;
type CreateConditionFn = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
type FindAllFn =
    unsafe extern "system" fn(*mut c_void, i32, *mut c_void, *mut *mut c_void) -> HRESULT;
type GetIntFn = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;
type GetBstrFn = unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT;
type GetRectFn = unsafe extern "system" fn(*mut c_void, *mut RECT) -> HRESULT;
type GetElementFn = unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> HRESULT;

/// `E_FAIL`, for a call that succeeded but gave back no object.
const E_FAIL: HRESULT = 0x8000_4005_u32 as i32;
/// `RPC_E_CHANGED_MODE`: the thread is already initialised for another apartment model.
const RPC_E_CHANGED_MODE: HRESULT = 0x8001_0106_u32 as i32;

/// A rectangle in screen pixels: left, top, width and height.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One element of a window's native tree.
///
/// Support: Partial. A missing name is `None`, not an empty string.
#[derive(Clone, Debug, PartialEq)]
pub struct UiaNode {
    /// The ARIA-style role for the element's control type. See [`role_for_control_type`].
    pub role: &'static str,
    /// The UI Automation control type ID the role was taken from.
    pub control_type: i32,
    /// The accessible name, with runs of whitespace collapsed. `None` when it is empty.
    pub name: Option<String>,
    pub enabled: bool,
    /// The bounding rectangle, or `None` when the element has none (an all-zero rectangle).
    pub bounds: Option<Rect>,
}

/// The properties [`row_from`] reads from one element. The real implementation reads them through
/// `IUIAutomationElement`; a test implements the trait directly.
///
/// Support: Full.
pub trait ElementSource {
    /// The `UIA_*ControlTypeId` of the element.
    fn control_type(&self) -> Result<i32, HRESULT>;
    /// The accessible name, or `None` when the element has none.
    fn name(&self) -> Result<Option<String>, HRESULT>;
    fn enabled(&self) -> Result<bool, HRESULT>;
    /// The bounding rectangle, or `None` when the element has none.
    fn bounds(&self) -> Result<Option<Rect>, HRESULT>;
}

/// Builds the row for one element from its properties.
///
/// Support: Full. Unit tested through a fake [`ElementSource`].
pub fn row_from<E: ElementSource>(element: &E) -> Result<UiaNode, HRESULT> {
    let control_type = element.control_type()?;
    let name = element
        .name()?
        .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|n| !n.is_empty());
    Ok(UiaNode {
        role: role_for_control_type(control_type),
        control_type,
        name,
        enabled: element.enabled()?,
        bounds: element.bounds()?,
    })
}

/// The ARIA-style role for a `UIA_*ControlTypeId`. Unknown IDs map to `"unknown"`.
///
/// The names are the roles the DOM side uses, so a button in the page and a button in the native
/// tree compare as equal. Where UI Automation has no direct ARIA equivalent, the nearest role is
/// used: a split button is a button, a header item a columnheader, a title bar a titlebar.
///
/// Support: Full. Every control type in the SDK header has a role; a unit test checks that.
pub fn role_for_control_type(control_type: i32) -> &'static str {
    match control_type {
        50000 => "button",
        50001 => "calendar",
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "textbox",
        50005 => "link",
        50006 => "img",
        50007 => "listitem",
        50008 => "list",
        50009 => "menu",
        50010 => "menubar",
        50011 => "menuitem",
        50012 => "progressbar",
        50013 => "radio",
        50014 => "scrollbar",
        50015 => "slider",
        50016 => "spinbutton",
        50017 => "status",
        50018 => "tablist",
        50019 => "tab",
        50020 => "text",
        50021 => "toolbar",
        50022 => "tooltip",
        50023 => "tree",
        50024 => "treeitem",
        50025 => "custom",
        50026 => "group",
        50027 => "thumb",
        50028 => "grid",
        50029 => "row",
        50030 => "document",
        50031 => "button",
        50032 => "window",
        50033 => "pane",
        50034 => "group",
        50035 => "columnheader",
        50036 => "table",
        50037 => "titlebar",
        50038 => "separator",
        50039 => "group",
        50040 => "toolbar",
        _ => "unknown",
    }
}

/// The function in `slot` of `object`'s vtable, as the caller's signature `F`.
///
/// # Safety
/// `object` must be a live COM interface pointer, `slot` must be the index of a method of its
/// interface, and `F` must be the signature that method is declared with.
unsafe fn method<F: Copy>(object: *mut c_void, slot: usize) -> F {
    let vtbl = *(object as *const *const *const c_void);
    let function = *vtbl.add(slot);
    // SAFETY: the caller guarantees `slot` names a function pointer of signature `F`, so the
    // pointer read from the vtable has the same size and meaning as `F`.
    std::mem::transmute_copy::<*const c_void, F>(&function)
}

/// Initialises COM on this thread for the duration of a dump, if it is not already. The guard
/// balances the initialisation only when this call made it.
struct ComScope {
    initialised: bool,
}

impl ComScope {
    fn enter() -> Result<Self, HRESULT> {
        // SAFETY: plain COM initialisation with no reserved pointer.
        let hr = unsafe { CoInitializeEx(null_mut(), COINIT_APARTMENTTHREADED as u32) };
        match hr {
            // S_OK, or S_FALSE (already initialised on this thread): both need an uninitialise.
            0 | 1 => Ok(ComScope { initialised: true }),
            RPC_E_CHANGED_MODE => Ok(ComScope { initialised: false }),
            hr => Err(hr),
        }
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.initialised {
            // SAFETY: balances the successful CoInitializeEx in `enter`.
            unsafe { CoUninitialize() };
        }
    }
}

/// An `IUIAutomationElement` read through its getters.
struct UiaElement {
    element: ComPtr,
}

impl UiaElement {
    fn get_int(&self, slot: usize) -> Result<i32, HRESULT> {
        let mut out = 0i32;
        // SAFETY: `self.element` is a live IUIAutomationElement, and `slot` is a getter that
        // returns an int through its last argument.
        let hr = unsafe {
            let f: GetIntFn = method(self.element.as_ptr(), slot);
            f(self.element.as_ptr(), &mut out)
        };
        if hr < 0 {
            return Err(hr);
        }
        Ok(out)
    }
}

impl ElementSource for UiaElement {
    fn control_type(&self) -> Result<i32, HRESULT> {
        self.get_int(SLOT_GET_CURRENT_CONTROL_TYPE)
    }

    fn name(&self) -> Result<Option<String>, HRESULT> {
        let mut bstr: *mut u16 = null_mut();
        // SAFETY: `self.element` is live, and the getter writes a BSTR the caller owns.
        let hr = unsafe {
            let f: GetBstrFn = method(self.element.as_ptr(), SLOT_GET_CURRENT_NAME);
            f(self.element.as_ptr(), &mut bstr)
        };
        if hr < 0 {
            return Err(hr);
        }
        if bstr.is_null() {
            return Ok(None);
        }
        // SAFETY: the BSTR is NUL-terminated UTF-16 and stays allocated until SysFreeString below.
        let name = unsafe { read_wide(bstr) };
        // SAFETY: frees the BSTR that the getter allocated, exactly once.
        unsafe { SysFreeString(bstr) };
        Ok(Some(name))
    }

    fn enabled(&self) -> Result<bool, HRESULT> {
        // A BOOL: nonzero is true.
        Ok(self.get_int(SLOT_GET_CURRENT_IS_ENABLED)? != 0)
    }

    fn bounds(&self) -> Result<Option<Rect>, HRESULT> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // SAFETY: `self.element` is live; the getter writes a RECT into `rect`.
        let hr = unsafe {
            let f: GetRectFn = method(self.element.as_ptr(), SLOT_GET_CURRENT_BOUNDING_RECTANGLE);
            f(self.element.as_ptr(), &mut rect)
        };
        if hr < 0 {
            return Err(hr);
        }
        // An element with no rectangle (for example, one not on screen) reports all zeros.
        if rect.left == 0 && rect.top == 0 && rect.right == 0 && rect.bottom == 0 {
            return Ok(None);
        }
        Ok(Some(Rect {
            x: f64::from(rect.left),
            y: f64::from(rect.top),
            width: f64::from(rect.right - rect.left),
            height: f64::from(rect.bottom - rect.top),
        }))
    }
}

/// Takes ownership of an object that a successful call wrote out, or fails with `what`.
fn owned(raw: *mut c_void, what: &str) -> Result<ComPtr, Error> {
    // SAFETY: the caller passes the pointer a successful call wrote, which carries one reference.
    unsafe { ComPtr::from_owned(raw) }.ok_or_else(|| hr_error(what, E_FAIL))
}

/// Dumps the native accessibility tree under `hwnd`, the window's own element included.
///
/// UI Automation returns every descendant of the window's element in one array, so the order is
/// UI Automation's own. A property that cannot be read fails the whole dump rather than dropping
/// the element, because a dropped element would hide a missing control from the comparison.
///
/// Support: Partial. Windows only. Needs a desktop: a window that is not visible may have no
/// elements or no bounds.
pub fn dump(hwnd: HWND) -> Result<Vec<UiaNode>, Error> {
    walk(hwnd)
}

/// The walk behind [`dump`]. The window handle is only passed on here, never dereferenced by
/// this crate, so the public entry point does not need to be `unsafe`.
fn walk(hwnd: HWND) -> Result<Vec<UiaNode>, Error> {
    let _com = ComScope::enter().map_err(|hr| hr_error("CoInitializeEx", hr))?;

    let automation = create_automation().map_err(|hr| hr_error("CoCreateInstance", hr))?;

    let mut raw_root: *mut c_void = null_mut();
    // SAFETY: `automation` is live; the callee writes one reference to the window's element.
    let hr = unsafe {
        let f: ElementFromHandleFn = method(automation.as_ptr(), SLOT_ELEMENT_FROM_HANDLE);
        f(automation.as_ptr(), hwnd, &mut raw_root)
    };
    if hr < 0 {
        return Err(hr_error("ElementFromHandle", hr));
    }
    let root = owned(raw_root, "ElementFromHandle")?;

    let mut raw_condition: *mut c_void = null_mut();
    // SAFETY: `automation` is live; the callee writes one reference to a condition.
    let hr = unsafe {
        let f: CreateConditionFn = method(automation.as_ptr(), SLOT_CREATE_TRUE_CONDITION);
        f(automation.as_ptr(), &mut raw_condition)
    };
    if hr < 0 {
        return Err(hr_error("CreateTrueCondition", hr));
    }
    let condition = owned(raw_condition, "CreateTrueCondition")?;

    let mut raw_array: *mut c_void = null_mut();
    // SAFETY: `root` and `condition` are live; the callee writes one reference to an array.
    let hr = unsafe {
        let f: FindAllFn = method(root.as_ptr(), SLOT_FIND_ALL);
        f(
            root.as_ptr(),
            TREE_SCOPE_SUBTREE,
            condition.as_ptr(),
            &mut raw_array,
        )
    };
    if hr < 0 {
        return Err(hr_error("FindAll", hr));
    }
    let array = owned(raw_array, "FindAll")?;

    let mut len = 0i32;
    // SAFETY: `array` is live; `len` is a writable int.
    let hr = unsafe {
        let f: GetIntFn = method(array.as_ptr(), SLOT_GET_LENGTH);
        f(array.as_ptr(), &mut len)
    };
    if hr < 0 {
        return Err(hr_error("get_Length", hr));
    }

    let mut rows = Vec::with_capacity(len.max(0) as usize);
    for index in 0..len {
        let mut raw_element: *mut c_void = null_mut();
        // SAFETY: `array` is live and `index` is inside its length; the callee writes one
        // reference to an element.
        let hr = unsafe {
            let f: GetElementFn = method(array.as_ptr(), SLOT_GET_ELEMENT);
            f(array.as_ptr(), index, &mut raw_element)
        };
        if hr < 0 {
            return Err(hr_error("GetElement", hr));
        }
        let element = UiaElement {
            element: owned(raw_element, "GetElement")?,
        };
        let row = row_from(&element).map_err(|hr| hr_error("element properties", hr))?;
        rows.push(row);
    }
    Ok(rows)
}

/// Creates the UI Automation root object.
fn create_automation() -> Result<ComPtr, HRESULT> {
    let mut raw: *mut c_void = null_mut();
    // SAFETY: the GUIDs live for the call; `Guid` has the layout of a COM GUID.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_CUI_AUTOMATION as *const Guid as *const windows_sys::core::GUID,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IUI_AUTOMATION as *const Guid as *const windows_sys::core::GUID,
            &mut raw,
        )
    };
    if hr < 0 {
        return Err(hr);
    }
    // SAFETY: on success the object gave us one reference, which ComPtr now owns.
    unsafe { ComPtr::from_owned(raw) }.ok_or(E_FAIL)
}

fn hr_error(what: &str, hr: HRESULT) -> Error {
    Error::Backend(format!("UI Automation: {what} failed with 0x{hr:08X}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake element with fixed properties, for the row-building tests.
    struct Fake {
        control_type: i32,
        name: Option<String>,
        enabled: bool,
        bounds: Option<Rect>,
    }

    impl ElementSource for Fake {
        fn control_type(&self) -> Result<i32, HRESULT> {
            Ok(self.control_type)
        }
        fn name(&self) -> Result<Option<String>, HRESULT> {
            Ok(self.name.clone())
        }
        fn enabled(&self) -> Result<bool, HRESULT> {
            Ok(self.enabled)
        }
        fn bounds(&self) -> Result<Option<Rect>, HRESULT> {
            Ok(self.bounds)
        }
    }

    fn button(name: Option<&str>) -> Fake {
        Fake {
            control_type: 50000,
            name: name.map(str::to_owned),
            enabled: true,
            bounds: Some(Rect {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 24.0,
            }),
        }
    }

    #[test]
    fn a_button_row_carries_role_name_enabled_and_bounds() {
        let row = row_from(&button(Some("Save"))).unwrap();
        assert_eq!(row.role, "button");
        assert_eq!(row.control_type, 50000);
        assert_eq!(row.name.as_deref(), Some("Save"));
        assert!(row.enabled);
        assert_eq!(
            row.bounds,
            Some(Rect {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 24.0
            })
        );
    }

    #[test]
    fn names_are_whitespace_normalised_and_an_empty_name_is_none() {
        let spaced = row_from(&button(Some("  Save \t as "))).unwrap();
        assert_eq!(spaced.name.as_deref(), Some("Save as"));
        let blank = row_from(&button(Some("   "))).unwrap();
        assert_eq!(blank.name, None);
        let missing = row_from(&button(None)).unwrap();
        assert_eq!(missing.name, None);
    }

    #[test]
    fn an_element_without_bounds_reports_none_and_keeps_its_row() {
        let hidden = Fake {
            control_type: 50020,
            name: Some("Status".into()),
            enabled: false,
            bounds: None,
        };
        let row = row_from(&hidden).unwrap();
        assert_eq!(row.role, "text");
        assert!(!row.enabled);
        assert_eq!(row.bounds, None);
    }

    #[test]
    fn an_unknown_control_type_maps_to_unknown() {
        assert_eq!(role_for_control_type(12345), "unknown");
        assert_eq!(role_for_control_type(50025), "custom");
    }

    /// Every `UIA_*ControlTypeId` in the SDK header must have a role, so a control type added in a
    /// later SDK is noticed here.
    #[test]
    fn every_control_type_in_the_header_has_a_role() {
        let Some(header) = header_text() else {
            eprintln!("skipped: no UIAutomationClient.h found (set TPT_UIA_HEADER)");
            return;
        };
        let mut seen = 0;
        for line in header.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("const long UIA_") else {
                continue;
            };
            if rest.split_once("ControlTypeId").is_none() {
                continue;
            }
            // `const long UIA_ButtonControlTypeId\t=\t50000;`
            let value = line
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_end_matches(';')
                .trim();
            let id: i32 = value.parse().unwrap();
            assert_ne!(role_for_control_type(id), "unknown", "control type {id}");
            seen += 1;
        }
        assert_eq!(seen, 41, "the header lists 41 control types");
    }

    /// Finds the SDK header: TPT_UIA_HEADER, or the newest Windows Kits include folder.
    fn header_text() -> Option<String> {
        if let Some(path) = std::env::var_os("TPT_UIA_HEADER") {
            return std::fs::read_to_string(path).ok();
        }
        let root = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\Include");
        let mut versions: Vec<_> = std::fs::read_dir(root)
            .ok()?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        versions.sort();
        let newest = versions.pop()?;
        std::fs::read_to_string(newest.join("um").join("UIAutomationClient.h")).ok()
    }

    /// The method names of an interface in slot order, starting at slot 3 (after IUnknown). The
    /// body is read from the declaration line up to the closing `};`.
    fn interface_methods(header: &str, interface: &str) -> Vec<String> {
        let declaration = format!("{interface} : public");
        let mut lines = header
            .lines()
            .skip_while(|l| !l.trim_start().starts_with(&declaration));
        let first = lines
            .next()
            .unwrap_or_else(|| panic!("{interface} is not in the header"));
        assert!(first.trim_start().starts_with(&declaration));
        lines
            .take_while(|l| !l.trim_start().starts_with("};"))
            .filter_map(|line| {
                let line = line.trim();
                let rest = line.strip_prefix("virtual ")?;
                // Property getters carry a `/* [propget] */` marker before the return type.
                let rest = rest.strip_prefix("/* [propget] */ ").unwrap_or(rest);
                let rest = rest.strip_prefix("HRESULT STDMETHODCALLTYPE ")?;
                Some(rest.split('(').next()?.trim().to_owned())
            })
            .collect()
    }

    /// Checks each vtable slot this module calls against the SDK header: the interface, the
    /// header index and the method name. The class ID and the interface ID are checked too.
    #[test]
    fn used_slots_match_the_uia_header() {
        let Some(header) = header_text() else {
            eprintln!("skipped: no UIAutomationClient.h found (set TPT_UIA_HEADER)");
            return;
        };
        // (interface, slot, method name), one row per slot this module calls.
        const USED: [(&str, usize, &str); 9] = [
            (
                "IUIAutomation",
                SLOT_ELEMENT_FROM_HANDLE,
                "ElementFromHandle",
            ),
            (
                "IUIAutomation",
                SLOT_CREATE_TRUE_CONDITION,
                "CreateTrueCondition",
            ),
            ("IUIAutomationElement", SLOT_FIND_ALL, "FindAll"),
            (
                "IUIAutomationElement",
                SLOT_GET_CURRENT_CONTROL_TYPE,
                "get_CurrentControlType",
            ),
            (
                "IUIAutomationElement",
                SLOT_GET_CURRENT_NAME,
                "get_CurrentName",
            ),
            (
                "IUIAutomationElement",
                SLOT_GET_CURRENT_IS_ENABLED,
                "get_CurrentIsEnabled",
            ),
            (
                "IUIAutomationElement",
                SLOT_GET_CURRENT_BOUNDING_RECTANGLE,
                "get_CurrentBoundingRectangle",
            ),
            ("IUIAutomationElementArray", SLOT_GET_LENGTH, "get_Length"),
            ("IUIAutomationElementArray", SLOT_GET_ELEMENT, "GetElement"),
        ];
        for (interface, slot, method_name) in USED {
            let methods = interface_methods(&header, interface);
            // `methods` starts at slot 3; the first three slots are IUnknown.
            let found = methods.get(slot - 3).map(String::as_str);
            assert_eq!(found, Some(method_name), "{interface} slot {slot}");
        }
        assert!(
            header.contains("30cbe57d-d9d0-452a-ab13-7ac5ac4825ee"),
            "the IUIAutomation IID is in the header"
        );
        assert!(
            header.contains("ff48dba4-60ef-4201-aa87-54103eef594e"),
            "the CUIAutomation CLSID is in the header"
        );
        assert!(header.contains("const long UIA_NamePropertyId"));
    }

    /// Creates a top-level window with a button in it, dumps the window, and checks the button is
    /// in the dump. Skips when there is no desktop to create the window on.
    #[test]
    fn a_dump_of_a_window_with_a_button_contains_the_button() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_CHILD, WS_POPUP, WS_VISIBLE,
        };

        let class = crate::com::wide("STATIC");
        let button_class = crate::com::wide("BUTTON");
        let title = crate::com::wide("UIA test window");
        let label = crate::com::wide("Save");
        // SAFETY: the strings are NUL-terminated and live for each call. A null return means no
        // window could be created, which is the no-desktop case.
        let window = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                100,
                100,
                240,
                120,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if window.is_null() {
            eprintln!("skipped: no desktop to create a window on");
            return;
        }
        // SAFETY: the parent is the window created above, and it is destroyed with it.
        let child = unsafe {
            CreateWindowExW(
                0,
                button_class.as_ptr(),
                label.as_ptr(),
                WS_CHILD | WS_VISIBLE,
                10,
                10,
                80,
                24,
                window,
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        assert!(!child.is_null(), "the button could not be created");

        let rows = dump(window);
        // SAFETY: the window is ours and is destroyed exactly once, here.
        unsafe { DestroyWindow(window) };
        let rows = rows.expect("the dump succeeds on a desktop");
        assert!(
            rows.iter()
                .any(|r| r.role == "button" && r.name.as_deref() == Some("Save") && r.enabled),
            "no enabled button named Save in {rows:?}"
        );
    }
}
