// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! ESC/POS receipt printer commands. Each method appends bytes to a [`Receipt`].

use crate::{check_ascii, UnsupportedText};

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;

/// Horizontal alignment for the next lines.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Accumulates ESC/POS bytes for one receipt. Starts with a printer reset (`ESC @`).
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    bytes: Vec<u8>,
}

impl Default for Receipt {
    fn default() -> Self {
        Self::new()
    }
}

impl Receipt {
    /// Support: Full.
    pub fn new() -> Self {
        Receipt {
            bytes: vec![ESC, b'@'],
        }
    }

    /// Prints `text` followed by a line feed.
    ///
    /// Support: Full.
    pub fn line(&mut self, text: &str) -> Result<&mut Self, UnsupportedText> {
        check_ascii(text)?;
        self.bytes.extend_from_slice(text.as_bytes());
        self.bytes.push(b'\n');
        Ok(self)
    }

    /// Feeds `lines` blank lines.
    ///
    /// Support: Full.
    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.bytes.extend_from_slice(&[ESC, b'd', lines]);
        self
    }

    /// Turns emphasis on or off (`ESC E`).
    ///
    /// Support: Full.
    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.bytes.extend_from_slice(&[ESC, b'E', u8::from(on)]);
        self
    }

    /// Sets alignment for following lines (`ESC a`).
    ///
    /// Support: Full.
    pub fn align(&mut self, align: Align) -> &mut Self {
        let n = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        self.bytes.extend_from_slice(&[ESC, b'a', n]);
        self
    }

    /// Cuts the paper. `partial` leaves a small tab (`GS V 1`); otherwise a full cut (`GS V 0`).
    ///
    /// Support: Full.
    pub fn cut(&mut self, partial: bool) -> &mut Self {
        let mode = if partial { 1 } else { 0 };
        self.bytes.extend_from_slice(&[GS, b'V', mode]);
        self
    }

    /// Support: Full.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receipt_has_the_documented_bytes() {
        let mut r = Receipt::new();
        r.align(Align::Center)
            .bold(true)
            .line("SHOP")
            .unwrap()
            .bold(false)
            .align(Align::Left)
            .line("Tea 2.50")
            .unwrap()
            .feed(2)
            .cut(false);
        let bytes = r.into_bytes();
        assert_eq!(
            bytes,
            [
                &[ESC, b'@'][..],
                &[ESC, b'a', 1],
                &[ESC, b'E', 1],
                b"SHOP\n",
                &[ESC, b'E', 0],
                &[ESC, b'a', 0],
                b"Tea 2.50\n",
                &[ESC, b'd', 2],
                &[GS, b'V', 0],
            ]
            .concat()
        );
    }

    #[test]
    fn partial_cut_uses_mode_one() {
        let mut r = Receipt::new();
        r.cut(true);
        assert_eq!(&r.into_bytes()[2..], &[GS, b'V', 1]);
    }

    #[test]
    fn non_ascii_text_is_refused_without_writing_anything() {
        let mut r = Receipt::new();
        let before = r.clone();
        let err = r.line("café").unwrap_err();
        assert_eq!(err.character, 'é');
        assert_eq!(r, before);
    }

    #[test]
    fn control_characters_are_refused() {
        let mut r = Receipt::new();
        assert!(r.line("a\u{1b}b").is_err());
    }
}
