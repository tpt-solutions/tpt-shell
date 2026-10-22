// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Native file dialogs (spec M3): open a file, save a file, and pick a folder, through the
//! shell's `IFileDialog`.
//!
//! The vtables are written by hand for the slots used here. Every slot is checked against
//! the Windows SDK header (`ShObjIdl_core.h`) by a test when `TPT_WINDOWS_SDK_UM` points at
//! the SDK's `um` include folder. Dialogs are modal and block until the user answers, so the
//! tests only create, configure and read back items; they never show a dialog.
//!
//! `open_files` allows several files to be picked. `open_file` picks one. The message box is
//! the plain `MessageBoxW`, modal to its owner window.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows_sys::Win32::UI::Shell::SHCreateItemFromParsingName;
use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;

use crate::com::{take_co_string, wide, ComPtr, Guid, IUnknownVtbl, E_POINTER, HRESULT};

const CLSID_FILE_OPEN_DIALOG: Guid = Guid {
    data1: 0xDC1C_5A9C,
    data2: 0xE88A,
    data3: 0x4DDE,
    data4: [0xA5, 0xA1, 0x60, 0xF8, 0x2A, 0x20, 0xAE, 0xF7],
};
const CLSID_FILE_SAVE_DIALOG: Guid = Guid {
    data1: 0xC0B4_E2F3,
    data2: 0xBA21,
    data3: 0x4773,
    data4: [0x8D, 0xBA, 0x33, 0x5E, 0xC9, 0x46, 0xEB, 0x8B],
};
/// `IFileDialog`. Both dialog classes implement it, so one interface serves open, save and
/// folder pickers.
const IID_IFILE_DIALOG: Guid = Guid {
    data1: 0x42F8_5136,
    data2: 0xDB7E,
    data3: 0x439C,
    data4: [0x85, 0xF1, 0xE4, 0x07, 0x5D, 0x13, 0x5F, 0xC8],
};
const IID_ISHELL_ITEM: Guid = Guid {
    data1: 0x4382_6D1E,
    data2: 0xE718,
    data3: 0x42EE,
    data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE],
};

/// `IFileOpenDialog`, which adds `GetResults` to `IFileDialog`.
const IID_IFILE_OPEN_DIALOG: Guid = Guid {
    data1: 0xD57C_7288,
    data2: 0xD4AD,
    data3: 0x4768,
    data4: [0xBE, 0x02, 0x9D, 0x96, 0x95, 0x32, 0xD9, 0x60],
};

const FOS_PICKFOLDERS: u32 = 0x20;
const FOS_FORCEFILESYSTEM: u32 = 0x40;
const FOS_ALLOWMULTISELECT: u32 = 0x200;
/// Message box button and icon flags, and the answers `MessageBoxW` returns.
const MB_OK: u32 = 0x0000_0000;
const MB_OKCANCEL: u32 = 0x0000_0001;
const MB_YESNO: u32 = 0x0000_0004;
const MB_YESNOCANCEL: u32 = 0x0000_0003;
const MB_ICONWARNING: u32 = 0x0000_0030;
const MB_ICONERROR: u32 = 0x0000_0010;
const MB_ICONINFORMATION: u32 = 0x0000_0040;
const IDOK: i32 = 1;
const IDYES: i32 = 6;
const IDNO: i32 = 7;
const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
/// `HRESULT_FROM_WIN32(ERROR_CANCELLED)`: the user closed the dialog.
const CANCELLED: HRESULT = 0x8007_04C7_u32 as i32;

/// `COMDLG_FILTERSPEC`: a display name and a pattern such as `*.txt;*.md`.
#[repr(C)]
struct FilterSpec {
    name: *const u16,
    spec: *const u16,
}

/// Slots 0-2 are `IUnknown`. Slot 3 is `IModalWindow::Show`. Slots 4-26 are `IFileDialog`.
#[repr(C)]
struct FileDialogVtbl {
    base: IUnknownVtbl,
    show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
    set_file_types: unsafe extern "system" fn(*mut c_void, u32, *const FilterSpec) -> HRESULT,
    set_file_type_index: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    _before_options: [usize; 3],
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    _before_title: [usize; 6],
    set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
    _before_result: [usize; 2],
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

/// `IShellItem`: slots 0-2 `IUnknown`, 3 `BindToHandler`, 4 `GetParent`, 5 `GetDisplayName`.
#[repr(C)]
struct ShellItemVtbl {
    base: IUnknownVtbl,
    _before_display_name: [usize; 2],
    get_display_name: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> HRESULT,
}

/// `IFileOpenDialog`: GetResults = 27 (the slots after `IFileDialog`'s 23 methods, 4-26).
#[repr(C)]
struct OpenDialogVtbl {
    base: IUnknownVtbl,
    _before_results: [usize; 24],
    get_results: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

/// `IShellItemArray`: GetCount = 7, GetItemAt = 8 (its base methods take slots 3-6).
#[repr(C)]
struct ShellItemArrayVtbl {
    base: IUnknownVtbl,
    _before_count: [usize; 4],
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    get_item_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> HRESULT,
}

/// What a dialog should look like.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DialogOptions {
    pub title: String,
    /// (display name, pattern) pairs, such as ("Text", "*.txt"). Empty means all files.
    pub filters: Vec<(String, String)>,
}

fn to_cstring(text: &str) -> Vec<u16> {
    wide(text)
}

/// Creates a dialog object of `class` and applies `options` and `extra_flags`.
fn create_dialog(
    class: &Guid,
    options: &DialogOptions,
    extra_flags: u32,
) -> Result<ComPtr, HRESULT> {
    let mut raw: *mut c_void = null_mut();
    // SAFETY: the GUIDs live for the call; `Guid` has the layout of a COM GUID.
    let hr = unsafe {
        CoCreateInstance(
            class as *const Guid as *const windows_sys::core::GUID,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IFILE_DIALOG as *const Guid as *const windows_sys::core::GUID,
            &mut raw,
        )
    };
    if hr < 0 {
        return Err(hr);
    }
    // SAFETY: on success the shell gave us one reference, which ComPtr now owns.
    let dialog = unsafe { ComPtr::from_owned(raw) }.ok_or(E_POINTER)?;
    let vt: &FileDialogVtbl = dialog.vtbl();

    let title = to_cstring(&options.title);
    // SAFETY: `title` is NUL-terminated and outlives the call.
    check(unsafe { (vt.set_title)(dialog.as_ptr(), title.as_ptr()) })?;

    if !options.filters.is_empty() {
        // The buffers must outlive the call that copies them out of this function's scope.
        let names: Vec<Vec<u16>> = options.filters.iter().map(|(n, _)| to_cstring(n)).collect();
        let specs: Vec<Vec<u16>> = options.filters.iter().map(|(_, s)| to_cstring(s)).collect();
        let array: Vec<FilterSpec> = names
            .iter()
            .zip(&specs)
            .map(|(n, s)| FilterSpec {
                name: n.as_ptr(),
                spec: s.as_ptr(),
            })
            .collect();
        let count = u32::try_from(array.len()).map_err(|_| E_POINTER)?;
        // SAFETY: `array`, and the buffers it points at, are alive for the call.
        check(unsafe { (vt.set_file_types)(dialog.as_ptr(), count, array.as_ptr()) })?;
        // SAFETY: index 1 is the first filter (indices are 1-based).
        check(unsafe { (vt.set_file_type_index)(dialog.as_ptr(), 1) })?;
    }

    let mut flags = 0u32;
    // SAFETY: `flags` is a valid out-pointer.
    check(unsafe { (vt.get_options)(dialog.as_ptr(), &mut flags) })?;
    // SAFETY: plain value; the flags only change the dialog's behaviour.
    check(unsafe { (vt.set_options)(dialog.as_ptr(), flags | FOS_FORCEFILESYSTEM | extra_flags) })?;
    Ok(dialog)
}

fn check(hr: HRESULT) -> Result<(), HRESULT> {
    if hr < 0 {
        Err(hr)
    } else {
        Ok(())
    }
}

/// Shows the dialog modally and returns the chosen path, or `None` when cancelled.
fn show(dialog: &ComPtr, owner: HWND) -> Result<Option<PathBuf>, HRESULT> {
    let vt: &FileDialogVtbl = dialog.vtbl();
    // SAFETY: the dialog is live; `owner` is a window handle or null.
    let hr = unsafe { (vt.show)(dialog.as_ptr(), owner) };
    if hr == CANCELLED {
        return Ok(None);
    }
    check(hr)?;
    let mut raw: *mut c_void = null_mut();
    // SAFETY: `raw` is a valid out-pointer.
    check(unsafe { (vt.get_result)(dialog.as_ptr(), &mut raw) })?;
    // SAFETY: on success the dialog gave us one `IShellItem` reference.
    let item = unsafe { ComPtr::from_owned(raw) }.ok_or(E_POINTER)?;
    item_path(&item).map(Some)
}

/// The file-system path of a shell item.
fn item_path(item: &ComPtr) -> Result<PathBuf, HRESULT> {
    let vt: &ShellItemVtbl = item.vtbl();
    let mut name: *mut u16 = null_mut();
    // SAFETY: `name` is a valid out-pointer; on success the string is shell-allocated and
    // `take_co_string` frees it.
    check(unsafe { (vt.get_display_name)(item.as_ptr(), SIGDN_FILESYSPATH, &mut name) })?;
    // SAFETY: as above.
    let text = unsafe { take_co_string(name) };
    Ok(PathBuf::from(text))
}

/// Shows an open-file dialog. `None` means the user cancelled.
pub fn open_file(owner: HWND, options: &DialogOptions) -> Result<Option<PathBuf>, HRESULT> {
    let dialog = create_dialog(&CLSID_FILE_OPEN_DIALOG, options, 0)?;
    show(&dialog, owner)
}

/// Shows a save-file dialog. `None` means the user cancelled.
pub fn save_file(owner: HWND, options: &DialogOptions) -> Result<Option<PathBuf>, HRESULT> {
    let dialog = create_dialog(&CLSID_FILE_SAVE_DIALOG, options, 0)?;
    show(&dialog, owner)
}

/// Shows a folder picker. `None` means the user cancelled.
pub fn pick_folder(owner: HWND, options: &DialogOptions) -> Result<Option<PathBuf>, HRESULT> {
    let dialog = create_dialog(&CLSID_FILE_OPEN_DIALOG, options, FOS_PICKFOLDERS)?;
    show(&dialog, owner)
}

/// Shows an open-file dialog that lets the user pick several files. `None` means the user
/// cancelled. The paths come back in the order the dialog lists them.
pub fn open_files(owner: HWND, options: &DialogOptions) -> Result<Option<Vec<PathBuf>>, HRESULT> {
    let dialog = create_dialog(&CLSID_FILE_OPEN_DIALOG, options, FOS_ALLOWMULTISELECT)?;
    if !show_modal(&dialog, owner)? {
        return Ok(None);
    }
    let open = dialog.query(&IID_IFILE_OPEN_DIALOG)?;
    let open_vt: &OpenDialogVtbl = open.vtbl();
    let mut raw: *mut c_void = null_mut();
    // SAFETY: `raw` is a valid out-pointer.
    check(unsafe { (open_vt.get_results)(open.as_ptr(), &mut raw) })?;
    // SAFETY: on success the dialog gave us one `IShellItemArray` reference.
    let items = unsafe { ComPtr::from_owned(raw) }.ok_or(E_POINTER)?;
    let items_vt: &ShellItemArrayVtbl = items.vtbl();
    let mut count: u32 = 0;
    // SAFETY: `count` is a valid out-pointer.
    check(unsafe { (items_vt.get_count)(items.as_ptr(), &mut count) })?;
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut raw_item: *mut c_void = null_mut();
        // SAFETY: `index` is below the count the array reported; `raw_item` is a valid out-pointer.
        check(unsafe { (items_vt.get_item_at)(items.as_ptr(), index, &mut raw_item) })?;
        // SAFETY: on success the array gave us one `IShellItem` reference.
        let item = unsafe { ComPtr::from_owned(raw_item) }.ok_or(E_POINTER)?;
        paths.push(item_path(&item)?);
    }
    Ok(Some(paths))
}

/// Which buttons a message box shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buttons {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

/// The icon a message box shows, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    None,
    Info,
    Warning,
    Error,
}

/// The button the user pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Ok,
    Cancel,
    Yes,
    No,
}

/// The `MessageBoxW` style flags for a button set and icon.
pub fn message_style(buttons: Buttons, icon: Icon) -> u32 {
    let b = match buttons {
        Buttons::Ok => MB_OK,
        Buttons::OkCancel => MB_OKCANCEL,
        Buttons::YesNo => MB_YESNO,
        Buttons::YesNoCancel => MB_YESNOCANCEL,
    };
    let i = match icon {
        Icon::None => 0,
        Icon::Info => MB_ICONINFORMATION,
        Icon::Warning => MB_ICONWARNING,
        Icon::Error => MB_ICONERROR,
    };
    b | i
}

/// Maps a `MessageBoxW` result to the answer. Codes the box cannot give map to `Cancel`.
pub fn answer_of(code: i32) -> Answer {
    match code {
        IDOK => Answer::Ok,
        IDYES => Answer::Yes,
        IDNO => Answer::No,
        _ => Answer::Cancel,
    }
}

/// Shows the dialog modally. Returns `false` when the user cancelled.
fn show_modal(dialog: &ComPtr, owner: HWND) -> Result<bool, HRESULT> {
    let vt: &FileDialogVtbl = dialog.vtbl();
    // SAFETY: the dialog is live; `owner` is a window handle or null.
    let hr = unsafe { (vt.show)(dialog.as_ptr(), owner) };
    if hr == CANCELLED {
        return Ok(false);
    }
    check(hr)?;
    Ok(true)
}

/// Shows a modal message box owned by `owner` (or by no window) and waits for the answer.
pub fn message_box(owner: HWND, title: &str, text: &str, buttons: Buttons, icon: Icon) -> Answer {
    answer_of(message_box_code(
        owner,
        &to_cstring(title),
        &to_cstring(text),
        message_style(buttons, icon),
    ))
}

/// Calls `MessageBoxW`. Both strings are NUL-terminated UTF-16 buffers. A failed call returns 0.
fn message_box_code(owner: HWND, title: &[u16], text: &[u16], style: u32) -> i32 {
    // SAFETY: both buffers are NUL-terminated and live across the call; `owner` is a window
    // handle or null.
    unsafe { MessageBoxW(owner, text.as_ptr(), title.as_ptr(), style) }
}
/// Creates a shell item for an existing path and reads its path back. Used to check the
/// `IShellItem` slots without showing a dialog.
pub fn path_round_trip(path: &std::path::Path) -> Result<PathBuf, HRESULT> {
    let text = wide(&path.to_string_lossy());
    let mut raw: *mut c_void = null_mut();
    // SAFETY: `text` is NUL-terminated; the IID lives for the call; no bind context is used.
    let hr = unsafe {
        SHCreateItemFromParsingName(
            text.as_ptr(),
            null_mut(),
            &IID_ISHELL_ITEM as *const Guid as *const windows_sys::core::GUID,
            &mut raw,
        )
    };
    check(hr)?;
    // SAFETY: on success the call gave us one reference.
    let item = unsafe { ComPtr::from_owned(raw) }.ok_or(E_POINTER)?;
    item_path(&item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::interactive_desktop_available;

    #[test]
    fn a_dialog_can_be_created_and_configured_without_showing_it() {
        if !interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        // SAFETY: standard apartment init for this test thread, balanced below.
        let hr = unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            )
        };
        assert!(hr >= 0);
        let options = DialogOptions {
            title: "Choose a file".into(),
            filters: vec![
                ("Text".into(), "*.txt".into()),
                ("All".into(), "*.*".into()),
            ],
        };
        for class in [&CLSID_FILE_OPEN_DIALOG, &CLSID_FILE_SAVE_DIALOG] {
            assert!(create_dialog(class, &options, 0).is_ok());
        }
        assert!(create_dialog(
            &CLSID_FILE_OPEN_DIALOG,
            &DialogOptions::default(),
            FOS_PICKFOLDERS
        )
        .is_ok());
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
    }

    #[test]
    fn shell_items_carry_a_path_through_the_vtable() {
        if !interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        // SAFETY: standard apartment init for this test thread, balanced below.
        unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            )
        };
        let path = std::env::temp_dir().join(format!("tpt-dialog-{}.txt", std::process::id()));
        std::fs::write(&path, "x").unwrap();
        // The temp dir may use an 8.3 short name (`RUNNER~1`) while the shell returns the long
        // form, so compare against the canonical path with its `\\?\` prefix dropped.
        let canonical = std::fs::canonicalize(&path).unwrap();
        let expected = canonical
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase();
        let back = path_round_trip(&path).expect("shell item round trip");
        let _ = std::fs::remove_file(&path);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        assert_eq!(back.to_string_lossy().to_lowercase(), expected);
    }

    /// Method names of the interface declared with `decl`, in vtable order, including the
    /// methods of its base interface named in `decl` (`: public Base`).
    fn sdk_methods(text: &str, iface: &str, base_methods: &[&str]) -> Option<Vec<String>> {
        let at = text.find(&format!("{iface} : public"))?;
        let body_end = text[at..].find("};")? + at;
        let mut names: Vec<String> = base_methods.iter().map(|s| s.to_string()).collect();
        let mut rest = &text[at..body_end];
        while let Some(p) = rest.find("STDMETHODCALLTYPE ") {
            let tail = &rest[p + "STDMETHODCALLTYPE ".len()..];
            let name: String = tail
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            names.push(name);
            rest = &rest[p + 1..];
        }
        Some(names)
    }

    #[test]
    fn vtable_slots_match_the_sdk_header() {
        let Some(dir) = std::env::var_os("TPT_WINDOWS_SDK_UM") else {
            eprintln!("skipped: set TPT_WINDOWS_SDK_UM to the SDK's um include folder");
            return;
        };
        let text = std::fs::read_to_string(PathBuf::from(dir).join("ShObjIdl_core.h"))
            .expect("ShObjIdl_core.h should be readable");

        // IModalWindow contributes Show, the first method after IUnknown. IFileDialog's own
        // list follows. The header's list starts after IUnknown.
        let dialog = sdk_methods(&text, "IFileDialog", &["Show"]).expect("IFileDialog in header");
        let slot = |name: &str| dialog.iter().position(|n| n == name).expect(name) + 3;
        assert_eq!(slot("Show"), 3);
        assert_eq!(slot("SetFileTypes"), 4);
        assert_eq!(slot("SetFileTypeIndex"), 5);
        assert_eq!(slot("SetOptions"), 9);
        assert_eq!(slot("GetOptions"), 10);
        assert_eq!(slot("SetTitle"), 17);
        assert_eq!(slot("GetResult"), 20);

        let item = sdk_methods(&text, "IShellItem", &[]).expect("IShellItem in header");
        let item_slot = |name: &str| item.iter().position(|n| n == name).expect(name) + 3;
        assert_eq!(item_slot("GetDisplayName"), 5);

        // IFileOpenDialog adds its own methods after IFileDialog's 23 (slots 4-26).
        let open = sdk_methods(&text, "IFileOpenDialog", &[]).expect("IFileOpenDialog in header");
        assert_eq!(open.first().map(String::as_str), Some("GetResults"));
        let array = sdk_methods(&text, "IShellItemArray", &[]).expect("IShellItemArray in header");
        let array_slot = |name: &str| array.iter().position(|n| n == name).expect(name) + 3;
        assert_eq!(array_slot("GetCount"), 7);
        assert_eq!(array_slot("GetItemAt"), 8);
    }
}

#[cfg(test)]
mod message_tests {
    use super::*;
    use crate::window::interactive_desktop_available;

    #[test]
    fn button_sets_and_icons_map_to_message_box_flags() {
        assert_eq!(message_style(Buttons::Ok, Icon::None), 0x0);
        assert_eq!(message_style(Buttons::OkCancel, Icon::Info), 0x1 | 0x40);
        assert_eq!(message_style(Buttons::YesNo, Icon::Warning), 0x4 | 0x30);
        assert_eq!(message_style(Buttons::YesNoCancel, Icon::Error), 0x3 | 0x10);
    }

    #[test]
    fn message_box_codes_map_to_answers_and_unknown_codes_cancel() {
        assert_eq!(answer_of(1), Answer::Ok);
        assert_eq!(answer_of(6), Answer::Yes);
        assert_eq!(answer_of(7), Answer::No);
        assert_eq!(answer_of(2), Answer::Cancel);
        // MessageBoxW returns 0 when it fails; that must not read as a yes.
        assert_eq!(answer_of(0), Answer::Cancel);
    }

    #[test]
    fn a_multi_select_dialog_is_an_open_file_dialog() {
        if !interactive_desktop_available() {
            eprintln!("skipped: no interactive desktop in this session");
            return;
        }
        // SAFETY: standard apartment init for this test thread, balanced below.
        let hr = unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                std::ptr::null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            )
        };
        assert!(hr >= 0);
        let dialog = create_dialog(
            &CLSID_FILE_OPEN_DIALOG,
            &DialogOptions::default(),
            FOS_ALLOWMULTISELECT,
        )
        .expect("the open dialog should be created");
        assert!(dialog.query(&IID_IFILE_OPEN_DIALOG).is_ok());
        // The dialog must be released before the apartment is torn down.
        drop(dialog);
        // SAFETY: balances the successful CoInitializeEx above.
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
    }
}
