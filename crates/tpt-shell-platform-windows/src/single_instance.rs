// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Single instance (spec M1). The first launch of an app owns a named mutex and a hidden
//! message-only window. A later launch sends its arguments to that window with `WM_COPYDATA`
//! and then exits. The handler runs on the first instance's UI thread.

use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, RegisterClassExW,
    SendMessageTimeoutW, HWND_MESSAGE, SMTO_ABORTIFHUNG, WM_COPYDATA, WNDCLASSEXW,
};

use crate::com::wide;

/// Tag on the copied data, so other `WM_COPYDATA` messages are ignored ("TPSH").
const TAG: usize = 0x5450_5348;
/// Longest argument block accepted from a second launch.
const MAX_BYTES: usize = 64 * 1024;
/// How long a second launch waits for the first instance to answer.
const SEND_TIMEOUT_MS: u32 = 5_000;

type Handler = Box<dyn FnMut(Vec<String>)>;

thread_local! {
    static HANDLER: RefCell<Option<Handler>> = RefCell::new(None);
}

/// The result of claiming an app's single instance.
pub enum Claim {
    /// This launch is the first. Keep the guard for as long as the app runs.
    First(InstanceGuard),
    /// Another launch owns the app. Its arguments were handed to it.
    Forwarded,
}

/// Owns the app's instance mutex and its message window. Dropping it lets the next launch
/// become the first instance.
pub struct InstanceGuard {
    mutex: HANDLE,
    window: HWND,
}

impl InstanceGuard {
    /// Sets the function that receives the arguments of each later launch. It replaces an
    /// earlier one.
    pub fn on_forwarded(&mut self, handler: impl FnMut(Vec<String>) + 'static) {
        HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(handler)));
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: both handles were created in `claim` and are released exactly once here.
        unsafe {
            DestroyWindow(self.window);
            CloseHandle(self.mutex);
        }
        HANDLER.with(|h| *h.borrow_mut() = None);
    }
}

/// Claims the single instance of the app `app_id`. `args` are this launch's arguments. They are
/// sent to the first instance when there is one. `app_id` may use letters, digits, `-`, `.` and
/// `_` only.
pub fn claim(app_id: &str, args: &[String]) -> Result<Claim, String> {
    if app_id.is_empty()
        || !app_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
    {
        return Err(format!(
            "single-instance id {app_id:?} has characters that are not allowed"
        ));
    }
    let mutex_name = wide(&format!(r"Local\tpt-shell-{app_id}"));
    // SAFETY: default security, not initially owned, NUL-terminated name that lives for the call.
    let mutex = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
    if mutex.is_null() {
        return Err(format!("CreateMutexW failed: error {}", unsafe {
            GetLastError()
        }));
    }
    // SAFETY: read straight after CreateMutexW, which is what sets this error code.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let class = wide(&format!("tpt-shell-instance-{app_id}"));

    if existed {
        // SAFETY: the handle is ours; the other instance keeps its own.
        unsafe { CloseHandle(mutex) };
        forward(&class, args)?;
        return Ok(Claim::Forwarded);
    }
    match create_window(&class) {
        Ok(window) => Ok(Claim::First(InstanceGuard { mutex, window })),
        Err(err) => {
            // SAFETY: created above and not yet handed out.
            unsafe { CloseHandle(mutex) };
            Err(err)
        }
    }
}

/// Sends `args` to the first instance's window.
fn forward(class: &[u16], args: &[String]) -> Result<(), String> {
    let payload = encode(args);
    if payload.len() > MAX_BYTES {
        return Err("the arguments are too long to pass to the running instance".into());
    }
    // SAFETY: the class name is NUL-terminated and lives for the call. Message-only windows are
    // found by searching under HWND_MESSAGE.
    let target = unsafe { FindWindowExW(HWND_MESSAGE, null_mut(), class.as_ptr(), null()) };
    if target.is_null() {
        return Err("the running instance has no window to receive the arguments".into());
    }
    let data = COPYDATASTRUCT {
        dwData: TAG,
        cbData: payload.len() as u32,
        lpData: payload.as_ptr() as *mut c_void,
    };
    let mut result: usize = 0;
    // SAFETY: `data` and `payload` live until the call returns, and the receiver only reads
    // them during the call. The timeout stops a hung first instance from blocking this launch.
    let answered = unsafe {
        SendMessageTimeoutW(
            target,
            WM_COPYDATA,
            0,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
    if answered == 0 {
        Err("the running instance did not answer".into())
    } else {
        Ok(())
    }
}

fn create_window(class: &[u16]) -> Result<HWND, String> {
    // SAFETY: the module handle is for this executable.
    let hinstance = unsafe { GetModuleHandleW(null()) };
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(instance_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
        hIconSm: null_mut(),
    };
    // SAFETY: `wc` and the name it points to live for the call. A failure means the class exists
    // already, and CreateWindowExW reports any real error.
    unsafe { RegisterClassExW(&wc) };
    // SAFETY: the class is registered; the window is message-only and never shown.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            hinstance,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err("CreateWindowExW (instance window) failed".into());
    }
    Ok(hwnd)
}

/// Window procedure of the instance window. Only `WM_COPYDATA` with our tag does anything.
unsafe extern "system" fn instance_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_COPYDATA {
        // SAFETY: for WM_COPYDATA, lParam is a COPYDATASTRUCT that the sender keeps alive during
        // the call.
        let data = &*(lparam as *const COPYDATASTRUCT);
        if data.dwData != TAG || data.lpData.is_null() || data.cbData as usize > MAX_BYTES {
            return 0;
        }
        // SAFETY: lpData points at cbData readable bytes for the duration of this call.
        let bytes = std::slice::from_raw_parts(data.lpData as *const u8, data.cbData as usize);
        let args = decode(bytes);
        // A panic in the handler must not unwind across this extern function.
        let _ = catch_unwind(AssertUnwindSafe(|| {
            HANDLER.with(|h| {
                if let Some(handler) = h.borrow_mut().as_mut() {
                    handler(args);
                }
            });
        }));
        return 1;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Each argument as UTF-8, followed by a NUL. The NUL keeps empty arguments.
fn encode(args: &[String]) -> Vec<u8> {
    args.iter()
        .flat_map(|arg| arg.bytes().chain(std::iter::once(0)))
        .collect()
}

fn decode(bytes: &[u8]) -> Vec<String> {
    let mut parts: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    // Every argument ends with a NUL, so the split leaves one empty piece at the end.
    parts.pop();
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn arguments_survive_the_encoding_including_empty_ones() {
        let args = vec!["--open".to_string(), String::new(), "a b.txt".to_string()];
        assert_eq!(decode(&encode(&args)), args);
        assert!(decode(&encode(&[])).is_empty());
    }

    #[test]
    fn an_id_with_unsafe_characters_is_refused() {
        assert!(claim("bad\\id", &[]).is_err());
        assert!(claim("", &[]).is_err());
    }

    #[test]
    fn a_second_launch_hands_its_arguments_to_the_first_and_the_name_is_free_after_drop() {
        let app_id = format!("test-{}", std::process::id());
        let mut first = match claim(&app_id, &["first".into()]).expect("first claim") {
            Claim::First(guard) => guard,
            Claim::Forwarded => panic!("the first claim should own the app"),
        };
        let seen: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        first.on_forwarded(move |args| sink.lock().expect("sink").push(args));

        let second = claim(&app_id, &["--open".into(), "a b.txt".into(), String::new()])
            .expect("second claim");
        assert!(matches!(second, Claim::Forwarded));
        assert_eq!(
            seen.lock().expect("seen").as_slice(),
            &[vec![
                "--open".to_string(),
                "a b.txt".to_string(),
                String::new()
            ]]
        );

        drop(first);
        assert!(matches!(
            claim(&app_id, &[]).expect("claim after drop"),
            Claim::First(_)
        ));
    }
}
