// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Reads the Windows accessibility settings that the page can react to (spec §6.4, M3).
//!
//! High contrast and animations come from `SystemParametersInfoW`. Transparency effects have
//! no system parameter, so they come from the per-user `Personalize` key, the same place
//! Settings writes them.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

use tpt_shell_core::theme::Signals;
use windows_sys::core::BOOL;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST,
};

use crate::com::wide;

/// The current settings. A setting that cannot be read counts as off, so a failed query never
/// stops a window from opening.
pub fn current() -> Signals {
    Signals {
        high_contrast: high_contrast(),
        reduced_motion: !client_area_animation(),
        reduced_transparency: !transparency_effects(),
    }
}

/// Whether high contrast is on (`SPI_GETHIGHCONTRAST`).
fn high_contrast() -> bool {
    let mut settings = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: null_mut(),
    };
    // SAFETY: `settings` is a live HIGHCONTRASTW whose `cbSize` matches its size, which is
    // what the system expects for this action.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            settings.cbSize,
            &mut settings as *mut HIGHCONTRASTW as *mut c_void,
            0,
        )
    };
    ok != 0 && settings.dwFlags & HCF_HIGHCONTRASTON != 0
}

/// Whether Windows animations are on (`SPI_GETCLIENTAREAANIMATION`). Defaults to on when the
/// query fails, since that is the Windows default.
fn client_area_animation() -> bool {
    let mut enabled: BOOL = 1;
    // SAFETY: `enabled` is a BOOL, the size the system writes for this action.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            &mut enabled as *mut BOOL as *mut c_void,
            0,
        )
    };
    ok == 0 || enabled != 0
}

/// Whether transparency effects are on. Defaults to on when the value is missing, which is
/// the Windows default.
fn transparency_effects() -> bool {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("EnableTransparency");
    let mut data: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: the strings are NUL-terminated and live for the call; `data` and `size` are
    // valid for the sizes given.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    status != 0 || data != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_can_be_read_on_this_machine() {
        // The values depend on the machine; the point is that every query completes and the
        // result is consistent: reduced transparency is the negation of the transparency read.
        let signals = current();
        assert_eq!(signals.reduced_transparency, !transparency_effects());
        assert_eq!(signals.reduced_motion, !client_area_animation());
    }
}
