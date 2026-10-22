// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Device support (spec §10.2, M7): identifiers and scopes, the transport trait, the hub that
//! opens devices under a scope and audits their use, virtual devices for tests, and the printer
//! and label builders.
//!
//! Text is ASCII. A code page is not chosen for the device, so non-ASCII text is refused
//! rather than printed as the wrong glyphs.

/// Support: Full. Parsers for hidraw uevent, sysfs ids, usbfs descriptors and ioctl numbers.
pub mod descriptors;
/// Support: Partial. The profile is the common plain-text one. Displays that need their own
/// command set (brightness, cursor, custom characters) are not supported, and a display that
/// does not move to the second line on line feed shows the bottom line in the wrong place.
pub mod display;
/// Support: Full.
pub mod escpos;
/// Support: Partial. Polling, not OS events: `Watch::poll_native` lists devices through the platform
/// (macOS and Linux serial, hidraw, usbfs, IOKit HID; none on Windows) and reports the difference.
pub mod hotplug;
/// Support: Partial. Hub, scope check and audit are Full; `open_with` reaches real devices only
/// where `native` has a backend.
pub mod hub;
/// Support: Full.
pub mod id;
/// Support: Full.
pub mod jobs;
/// Support: Partial. Real devices through the OS: see the table in the module docs.
pub mod native;
/// Support: Partial. A timeout during a write that has already sent some bytes is an error, not
/// busy, because the bytes cannot be taken back. Name resolution uses the system resolver, which
/// can block for as long as the system takes to answer.
pub mod net;
/// Support: Full. Serial and open options, and the termios plan, as pure data.
pub mod options;
/// Support: Partial. Windows serial and spool; CUPS spool, termios serial, hidraw and usbfs (Linux)
/// and IOKit HID (macOS) on Unix, all compile-checked only from this workspace. No WinUSB, Windows
/// HID or IOKit USB.
pub mod platform;
/// Support: Partial. The line shape is generic; a scale that sends a different shape needs its own
/// parser.
pub mod scale;
/// Support: Partial. HID POS decodes only boot-protocol keyboard reports, so a scanner that needs
/// its own report descriptor is not handled.
pub mod scanner;
/// Support: Partial. Windows (COM, baud only) and macOS/Linux (termios, all line settings,
/// compile-checked, run on CI with a port). Discovery on macOS and Linux only.
pub mod serial;
/// Support: Partial. Windows: the winspool RAW datatype, tested. macOS and Linux go through CUPS lp
/// with -o raw; not run on those systems from this workspace. Other platforms return NotSupported.
pub mod spool;
/// Support: Full.
pub mod transport;
/// Support: Full.
pub mod virtual_devices;
/// Support: Full.
pub mod zpl;

/// Text the builders refuse: anything outside printable ASCII plus tab and newline.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsupportedText {
    pub character: char,
}

impl std::fmt::Display for UnsupportedText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "character {:?} is not printable ASCII; this builder does not choose a code page",
            self.character
        )
    }
}

impl std::error::Error for UnsupportedText {}

/// Checks that `text` is printable ASCII, tab, or newline.
///
/// Support: Full.
pub fn check_ascii(text: &str) -> Result<(), UnsupportedText> {
    match text
        .chars()
        .find(|c| !(c.is_ascii_graphic() || *c == ' ' || *c == '\t' || *c == '\n'))
    {
        Some(character) => Err(UnsupportedText { character }),
        None => Ok(()),
    }
}
