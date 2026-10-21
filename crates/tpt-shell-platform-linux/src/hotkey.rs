// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Global hotkeys on X11 (todo M5, decision 5). Partial: X11 only.
//!
//! Wayland does not let a client grab keys globally, so [`Hotkeys::open`] returns
//! [`Error::Unsupported`] when there is no X display. The keys are grabbed on the root window
//! through Xlib, which is loaded with `dlopen` like GTK, so the crate builds without it.
//!
//! [`parse`] turns `Ctrl+Shift+K` into an [`Accelerator`]. It runs on every platform and is
//! tested everywhere. A hotkey must have at least one modifier, so a bare letter can never be
//! grabbed and stolen from typing.

use std::ffi::{c_char, c_int, c_uchar, c_uint, c_ulong, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};

use tpt_shell_core::{Error, Result};

use crate::loader::SharedLibrary;

/// Identifies a registered hotkey. Returned by [`Hotkeys::register`] and reported by
/// [`Hotkeys::poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HotkeyId(pub u32);

/// A parsed hotkey: the modifiers and the key, named the way Xlib names keysyms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accelerator {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    /// Xlib keysym name, such as `k`, `F5`, `Return` or `space`.
    pub keysym: String,
}

/// Parses an accelerator such as `Ctrl+Shift+K` or `Super+F5`. Names are case-insensitive.
/// Refused: no modifier, two keys, an unknown key name, or a repeated modifier.
pub fn parse(text: &str) -> std::result::Result<Accelerator, String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = parts
        .split_last()
        .ok_or_else(|| "empty hotkey".to_string())?;
    let mut accel = Accelerator {
        ctrl: false,
        shift: false,
        alt: false,
        super_key: false,
        keysym: keysym_for(key).ok_or_else(|| format!("unknown key `{key}`"))?,
    };
    for m in mods {
        let slot = match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => &mut accel.ctrl,
            "shift" => &mut accel.shift,
            "alt" | "option" => &mut accel.alt,
            "super" | "cmd" | "command" | "meta" | "win" => &mut accel.super_key,
            other => return Err(format!("unknown modifier `{other}`")),
        };
        if *slot {
            return Err(format!("modifier `{m}` is given twice"));
        }
        *slot = true;
    }
    if !(accel.ctrl || accel.shift || accel.alt || accel.super_key) {
        return Err("a hotkey needs at least one modifier (Ctrl, Shift, Alt or Super)".to_string());
    }
    Ok(accel)
}

/// Maps a key name to the Xlib keysym name. `None` for a name it does not know.
fn keysym_for(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" => "space",
        "enter" | "return" => "Return",
        "tab" => "Tab",
        "esc" | "escape" => "Escape",
        "backspace" => "BackSpace",
        "delete" | "del" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Prior",
        "pagedown" => "Next",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    if key.len() == 1 {
        let c = key.chars().next()?;
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_lowercase().to_string());
        }
        return None;
    }
    // F1 to F35, the Xlib names.
    if let Some(n) = lower.strip_prefix('f') {
        if let Ok(n) = n.parse::<u8>() {
            if (1..=35).contains(&n) && !n.to_string().starts_with('0') {
                return Some(format!("F{n}"));
            }
        }
    }
    None
}

// Xlib modifier bits (X.h).
const SHIFT_MASK: c_uint = 1 << 0;
const LOCK_MASK: c_uint = 1 << 1;
const CONTROL_MASK: c_uint = 1 << 2;
const MOD1_MASK: c_uint = 1 << 3; // Alt
const MOD2_MASK: c_uint = 1 << 4; // NumLock
const MOD4_MASK: c_uint = 1 << 6; // Super
const KEY_PRESS: c_int = 2;
const GRAB_MODE_ASYNC: c_int = 1;

impl Accelerator {
    /// The modifier bits for `XGrabKey`.
    fn x_modifiers(&self) -> c_uint {
        let mut mask = 0;
        if self.shift {
            mask |= SHIFT_MASK;
        }
        if self.ctrl {
            mask |= CONTROL_MASK;
        }
        if self.alt {
            mask |= MOD1_MASK;
        }
        if self.super_key {
            mask |= MOD4_MASK;
        }
        mask
    }
}

type OpenDisplayFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type CloseDisplayFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type RootWindowFn = unsafe extern "C" fn(*mut c_void) -> c_ulong;
type StringToKeysymFn = unsafe extern "C" fn(*const c_char) -> c_ulong;
type KeysymToKeycodeFn = unsafe extern "C" fn(*mut c_void, c_ulong) -> c_uchar;
type GrabKeyFn =
    unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong, c_int, c_int, c_int) -> c_int;
type UngrabKeyFn = unsafe extern "C" fn(*mut c_void, c_int, c_uint, c_ulong) -> c_int;
type PendingFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type NextEventFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type SyncFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type ErrorHandlerFn = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;
type SetErrorHandlerFn = unsafe extern "C" fn(ErrorHandlerFn) -> Option<ErrorHandlerFn>;

/// The Xlib entry points used here, loaded from `libX11.so.6`.
struct Xlib {
    _lib: SharedLibrary,
    open_display: OpenDisplayFn,
    close_display: CloseDisplayFn,
    root_window: RootWindowFn,
    string_to_keysym: StringToKeysymFn,
    keysym_to_keycode: KeysymToKeycodeFn,
    grab_key: GrabKeyFn,
    ungrab_key: UngrabKeyFn,
    pending: PendingFn,
    next_event: NextEventFn,
    sync: SyncFn,
    set_error_handler: SetErrorHandlerFn,
}

impl Xlib {
    fn load() -> Result<Xlib> {
        let lib =
            SharedLibrary::open(&["libX11.so.6"]).map_err(|e| Error::Backend(e.to_string()))?;
        // SAFETY: each type below is the C prototype of the named Xlib function (Xlib.h).
        unsafe {
            Ok(Xlib {
                open_display: lib.symbol(XOPEN).map_err(backend)?,
                close_display: lib.symbol(XCLOSE).map_err(backend)?,
                root_window: lib.symbol(XROOT).map_err(backend)?,
                string_to_keysym: lib.symbol(XSTRING_TO_KEYSYM).map_err(backend)?,
                keysym_to_keycode: lib.symbol(XKEYSYM_TO_KEYCODE).map_err(backend)?,
                grab_key: lib.symbol(XGRAB_KEY).map_err(backend)?,
                ungrab_key: lib.symbol(XUNGRAB_KEY).map_err(backend)?,
                pending: lib.symbol(XPENDING).map_err(backend)?,
                next_event: lib.symbol(XNEXT_EVENT).map_err(backend)?,
                sync: lib.symbol(XSYNC).map_err(backend)?,
                set_error_handler: lib.symbol(XSET_ERROR_HANDLER).map_err(backend)?,
                _lib: lib,
            })
        }
    }
}

const XOPEN: &str = "XOpenDisplay";
const XCLOSE: &str = "XCloseDisplay";
const XROOT: &str = "XDefaultRootWindow";
const XSTRING_TO_KEYSYM: &str = "XStringToKeysym";
const XKEYSYM_TO_KEYCODE: &str = "XKeysymToKeycode";
const XGRAB_KEY: &str = "XGrabKey";
const XUNGRAB_KEY: &str = "XUngrabKey";
const XPENDING: &str = "XPending";
const XNEXT_EVENT: &str = "XNextEvent";
const XSYNC: &str = "XSync";
const XSET_ERROR_HANDLER: &str = "XSetErrorHandler";

fn backend(e: crate::loader::LoadError) -> Error {
    Error::Backend(e.to_string())
}

/// Set by the Xlib error handler when a grab fails (for example, another client holds the key).
static GRAB_FAILED: AtomicBool = AtomicBool::new(false);

/// Xlib error handler. Xlib's default one exits the process, so this one only records the error.
///
/// # Safety
/// Called by Xlib with a valid display and error event. It reads nothing from them.
unsafe extern "C" fn record_error(_display: *mut c_void, _event: *mut c_void) -> c_int {
    GRAB_FAILED.store(true, Ordering::SeqCst);
    0
}

/// One grabbed key. A grab is made for each combination of NumLock and CapsLock, so the hotkey
/// still fires when either is on.
struct Binding {
    id: HotkeyId,
    keycode: c_int,
    modifiers: c_uint,
}

/// Global hotkeys on the X11 display. Use from the UI thread only.
pub struct Hotkeys {
    xlib: Xlib,
    display: *mut c_void,
    root: c_ulong,
    bindings: Vec<Binding>,
    next_id: u32,
}

impl Hotkeys {
    /// Connects to the X display. Refused with [`Error::Unsupported`] when there is none, so a
    /// Wayland-only session reports that global hotkeys are not available.
    pub fn open() -> Result<Hotkeys> {
        let display_var = std::env::var("DISPLAY").unwrap_or_default();
        if display_var.is_empty() {
            return Err(Error::Unsupported {
                feature: "global hotkeys (X11 only; no DISPLAY, so this is a Wayland-only session)",
            });
        }
        let xlib = Xlib::load()?;
        let name =
            CString::new(display_var).map_err(|_| Error::Backend("DISPLAY holds a NUL".into()))?;
        // SAFETY: `name` is a valid NUL-terminated string that outlives the call.
        let display = unsafe { (xlib.open_display)(name.as_ptr()) };
        if display.is_null() {
            return Err(Error::Backend(
                "cannot open the X display named by DISPLAY".into(),
            ));
        }
        // SAFETY: `display` is a live display; the handler has the Xlib error signature.
        let root = unsafe { (xlib.root_window)(display) };
        // SAFETY: installing the handler only changes which function Xlib calls on errors.
        unsafe { (xlib.set_error_handler)(record_error) };
        Ok(Hotkeys {
            xlib,
            display,
            root,
            bindings: Vec::new(),
            next_id: 1,
        })
    }

    /// Grabs the accelerator. Fails when the text does not parse, the key name is unknown, or
    /// another client already holds the key.
    pub fn register(&mut self, accelerator: &str) -> Result<HotkeyId> {
        let accel = parse(accelerator)
            .map_err(|m| Error::Backend(format!("hotkey `{accelerator}`: {m}")))?;
        let keysym_name = CString::new(accel.keysym.as_str())
            .map_err(|_| Error::Backend("bad key name".into()))?;
        // SAFETY: the name is a valid C string for the call.
        let keysym = unsafe { (self.xlib.string_to_keysym)(keysym_name.as_ptr()) };
        if keysym == 0 {
            return Err(Error::Backend(format!(
                "Xlib does not know the key `{}`",
                accel.keysym
            )));
        }
        // SAFETY: the display is live (it is closed only in Drop).
        let keycode = unsafe { (self.xlib.keysym_to_keycode)(self.display, keysym) } as c_int;
        if keycode == 0 {
            return Err(Error::Backend(format!(
                "no key on this keyboard for `{}`",
                accel.keysym
            )));
        }
        let id = HotkeyId(self.next_id);
        self.next_id += 1;
        GRAB_FAILED.store(false, Ordering::SeqCst);
        for extra in [0, LOCK_MASK, MOD2_MASK, LOCK_MASK | MOD2_MASK] {
            let modifiers = accel.x_modifiers() | extra;
            // SAFETY: the display and root window are live. Errors arrive via `record_error`.
            unsafe {
                (self.xlib.grab_key)(
                    self.display,
                    keycode,
                    modifiers,
                    self.root,
                    0,
                    GRAB_MODE_ASYNC,
                    GRAB_MODE_ASYNC,
                );
            }
            self.bindings.push(Binding {
                id,
                keycode,
                modifiers,
            });
        }
        // SAFETY: flushes the requests so an error for them is reported before we check.
        unsafe { (self.xlib.sync)(self.display, 0) };
        if GRAB_FAILED.load(Ordering::SeqCst) {
            self.remove(id);
            return Err(Error::Backend(format!(
                "`{accelerator}` is already used by another program"
            )));
        }
        Ok(id)
    }

    /// Releases a hotkey. An unknown id does nothing.
    pub fn unregister(&mut self, id: HotkeyId) {
        self.remove(id);
    }

    fn remove(&mut self, id: HotkeyId) {
        let (mine, rest): (Vec<Binding>, Vec<Binding>) = std::mem::take(&mut self.bindings)
            .into_iter()
            .partition(|b| b.id == id);
        self.bindings = rest;
        for b in mine {
            // SAFETY: the display is live and the binding was grabbed with these values.
            unsafe { (self.xlib.ungrab_key)(self.display, b.keycode, b.modifiers, self.root) };
        }
    }

    /// Returns the hotkeys pressed since the last call. Does not block.
    pub fn poll(&mut self) -> Vec<HotkeyId> {
        let mut fired = Vec::new();
        // SAFETY: `XEvent` is a union of at most 192 bytes on 64-bit Linux. The buffer is larger
        // than that, 8-byte aligned, and Xlib writes only into it.
        let mut event = [0u64; 24];
        loop {
            // SAFETY: the display is live.
            if unsafe { (self.xlib.pending)(self.display) } <= 0 {
                break;
            }
            // SAFETY: `event` is large enough for any XEvent.
            unsafe { (self.xlib.next_event)(self.display, event.as_mut_ptr().cast()) };
            let bytes = event.as_ptr().cast::<u8>();
            // SAFETY: XKeyEvent has `type` at offset 0, `state` at 80 and `keycode` at 84 on
            // 64-bit Linux, all inside the buffer.
            let (kind, state, keycode) = unsafe {
                (
                    *bytes.cast::<c_int>(),
                    *bytes.add(80).cast::<c_uint>(),
                    *bytes.add(84).cast::<c_uint>(),
                )
            };
            if kind != KEY_PRESS {
                continue;
            }
            // NumLock and CapsLock do not change which hotkey was meant.
            let state = state & !(LOCK_MASK | MOD2_MASK);
            for b in &self.bindings {
                let same_key = b.keycode as c_uint == keycode;
                if same_key
                    && (b.modifiers & !(LOCK_MASK | MOD2_MASK)) == state
                    && !fired.contains(&b.id)
                {
                    fired.push(b.id);
                }
            }
        }
        fired
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for b in std::mem::take(&mut self.bindings) {
            // SAFETY: the display is still open here; it is closed below.
            unsafe { (self.xlib.ungrab_key)(self.display, b.keycode, b.modifiers, self.root) };
        }
        // SAFETY: the display came from XOpenDisplay and is not used after this.
        unsafe { (self.xlib.close_display)(self.display) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys_case_insensitively() {
        let a = parse("ctrl+SHIFT+k").unwrap();
        assert!(a.ctrl && a.shift && !a.alt && !a.super_key);
        assert_eq!(a.keysym, "k");
        let f = parse("Super+F5").unwrap();
        assert!(f.super_key);
        assert_eq!(f.keysym, "F5");
        assert_eq!(parse("Alt+Enter").unwrap().keysym, "Return");
        assert_eq!(parse("Ctrl+PageDown").unwrap().keysym, "Next");
        assert_eq!(parse("Ctrl+9").unwrap().keysym, "9");
    }

    #[test]
    fn refuses_bad_accelerators() {
        assert!(parse("k").is_err(), "a bare key is refused");
        assert!(
            parse("Ctrl+Shift").is_err(),
            "a modifier alone is not a key"
        );
        assert!(
            parse("Ctrl+Ctrl+k").is_err(),
            "a repeated modifier is refused"
        );
        assert!(parse("Hyper+k").is_err(), "unknown modifier");
        assert!(parse("Ctrl+F0").is_err());
        assert!(parse("Ctrl+F36").is_err());
        assert!(parse("Ctrl+ab").is_err(), "unknown multi-letter key");
        assert!(parse("Ctrl+-").is_err());
    }

    #[test]
    fn x_modifier_bits_follow_xlib() {
        let a = parse("Ctrl+Alt+Shift+Super+a").unwrap();
        assert_eq!(
            a.x_modifiers(),
            SHIFT_MASK | CONTROL_MASK | MOD1_MASK | MOD4_MASK
        );
    }

    /// Runs only where an X display exists (a desktop session or Xvfb). Skips cleanly otherwise.
    #[test]
    fn opening_without_a_display_is_unsupported_not_a_panic() {
        if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) {
            // A display is present: opening must work or fail with a backend error, not panic.
            let _ = Hotkeys::open();
            return;
        }
        assert!(matches!(Hotkeys::open(), Err(Error::Unsupported { .. })));
    }
}
