// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Operating-system device code. Everything with `unsafe` or FFI lives in these modules, each
//! behind a `cfg` for its platform, so other platforms still build.

#[cfg(target_os = "linux")]
/// Support: Partial. hidraw; compile-checked; the runtime test skips without a device.
pub mod linux_hid;
#[cfg(all(
    target_os = "linux",
    not(any(
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "sparc",
        target_arch = "sparc64"
    ))
))]
/// Support: Partial. usbfs; compile-checked; the runtime test skips without a device.
pub mod linux_usb;
#[cfg(target_os = "macos")]
/// Support: Partial. IOKit HID; compile-checked only, never run on a Mac from this workspace.
pub mod macos_hid;
#[cfg(unix)]
/// Support: Partial. termios; compile-checked; the runtime test skips without a port.
pub mod unix_serial;
#[cfg(unix)]
/// Support: Partial. Goes through CUPS lp with -o raw, which needs CUPS installed; not run on
/// macOS/Linux here; compile-checked.
pub mod unix_spool;
#[cfg(windows)]
/// Support: Full.
pub mod win_serial;
#[cfg(windows)]
/// Support: Full.
pub mod win_spool;
