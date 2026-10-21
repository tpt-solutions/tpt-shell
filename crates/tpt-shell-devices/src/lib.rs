// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Printer and label builders (spec §10.2, helpers). These build bytes only; sending them to
//! a device is the transport's job (later milestones).
//!
//! Text is ASCII. A code page is not chosen for the device, so non-ASCII text is refused
//! rather than printed as the wrong glyphs.

pub mod escpos;
pub mod id;
pub mod zpl;

/// Text the builders refuse: anything outside printable ASCII plus tab and newline.
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
pub fn check_ascii(text: &str) -> Result<(), UnsupportedText> {
    match text
        .chars()
        .find(|c| !(c.is_ascii_graphic() || *c == ' ' || *c == '\t' || *c == '\n'))
    {
        Some(character) => Err(UnsupportedText { character }),
        None => Ok(()),
    }
}
