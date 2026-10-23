// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Customer displays (spec §10.2, M7): a two-line serial display driven by a generic ASCII
//! profile. A clear byte (`0x0C`) starts a new frame; the top line follows, then carriage return
//! and line feed, then the bottom line. Each line is padded or cut to the display's width.
//!
//! Support: Partial. The profile is the common plain-text one. Displays that need their own
//! command set (brightness, cursor, custom characters) are not supported, and a display that
//! does not move to the second line on line feed will show the bottom line in the wrong place.

use crate::transport::{DeviceError, Transport};

const CLEAR: u8 = 0x0C;

/// Why a frame was refused or not sent.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplayError {
    /// The width is outside 1 to 40 columns.
    BadWidth(usize),
    /// A character that is not printable ASCII. The display's code page is not chosen here.
    NotPrintable(char),
    /// The device refused the bytes.
    Device(DeviceError),
    /// The cursor is off the screen: the row is not 1 or 2, or the column is not 1 to the width.
    BadCursor { row: u8, column: usize },
    /// The text is wider than the columns left from the cursor to the end of the row. It is
    /// refused, not wrapped onto the next row.
    TooWide { text_columns: usize, room: usize },
}

impl std::fmt::Display for DisplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayError::BadWidth(w) => write!(f, "a display width of {w} is not 1 to 40 columns"),
            DisplayError::NotPrintable(c) => {
                write!(
                    f,
                    "{c:?} is not printable ASCII; the display's code page is not chosen"
                )
            }
            DisplayError::Device(e) => write!(f, "{e}"),
            DisplayError::BadCursor { row, column } => {
                write!(f, "cursor at row {row}, column {column} is off the display")
            }
            DisplayError::TooWide { text_columns, room } => write!(
                f,
                "text of {text_columns} columns does not fit in the {room} columns left on the row"
            ),
        }
    }
}

impl std::error::Error for DisplayError {}

impl From<DeviceError> for DisplayError {
    fn from(e: DeviceError) -> Self {
        DisplayError::Device(e)
    }
}

/// A two-line customer display, `columns` wide.
///
/// Support: Partial. The profile is the common plain-text one. Displays that need their own command
/// set (brightness, cursor, custom characters) are not supported, and a display that does not move
/// to the second line on line feed shows the bottom line in the wrong place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomerDisplay {
    columns: usize,
}

impl Default for CustomerDisplay {
    /// The common 2 by 20 display.
    fn default() -> Self {
        CustomerDisplay { columns: 20 }
    }
}

impl CustomerDisplay {
    /// Support: Partial. The profile is the common plain-text one. Displays that need their own
    /// command set (brightness, cursor, custom characters) are not supported, and a display that
    /// does not move to the second line on line feed shows the bottom line in the wrong place.
    pub fn new(columns: usize) -> Result<Self, DisplayError> {
        if (1..=40).contains(&columns) {
            Ok(CustomerDisplay { columns })
        } else {
            Err(DisplayError::BadWidth(columns))
        }
    }

    /// The bytes for one frame. Text longer than the width is cut; shorter text is padded so the
    /// old characters are cleared.
    ///
    /// Support: Partial. The profile is the common plain-text one. Displays that need their own
    /// command set (brightness, cursor, custom characters) are not supported, and a display that
    /// does not move to the second line on line feed shows the bottom line in the wrong place.
    pub fn frame(&self, top: &str, bottom: &str) -> Result<Vec<u8>, DisplayError> {
        let mut out = vec![CLEAR];
        out.extend(self.line(top)?);
        out.extend_from_slice(b"\r\n");
        out.extend(self.line(bottom)?);
        Ok(out)
    }

    /// Sends one frame to the display.
    ///
    /// Support: Partial. The profile is the common plain-text one. Displays that need their own
    /// command set (brightness, cursor, custom characters) are not supported, and a display that
    /// does not move to the second line on line feed shows the bottom line in the wrong place.
    pub fn show(
        &self,
        transport: &mut dyn Transport,
        top: &str,
        bottom: &str,
    ) -> Result<(), DisplayError> {
        let bytes = self.frame(top, bottom)?;
        transport.write(&bytes)?;
        Ok(())
    }

    fn line(&self, text: &str) -> Result<Vec<u8>, DisplayError> {
        if let Some(c) = text.chars().find(|c| !(c.is_ascii_graphic() || *c == ' ')) {
            return Err(DisplayError::NotPrintable(c));
        }
        let mut bytes: Vec<u8> = text.bytes().take(self.columns).collect();
        bytes.resize(self.columns, b' ');
        Ok(bytes)
    }
}

const ESC: u8 = 0x1B;

/// Builds the bytes for one screen on a two-line display, one command at a time: clear, move the
/// cursor, write text. The builder follows the cursor, so each text starts where the last one
/// ended, and text that would run past the last column is refused.
///
/// The cursor command is `ESC`, `L`, row, column, with row 1 or 2 and column 1 up to the width.
/// This is this crate's profile for the display, not a command taken from a vendor manual, so it
/// must be checked against the display before it is relied on.
///
/// Support: Partial. The cursor command is this crate's own profile and is not checked against a
/// vendor's manual. Text is printable ASCII only, as in [`CustomerDisplay`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VfdScreen {
    columns: usize,
    column: usize,
    bytes: Vec<u8>,
}

impl VfdScreen {
    /// Starts a screen `columns` wide with the cursor at row 1, column 1.
    ///
    /// Support: Partial. The cursor command is this crate's own profile; see [`VfdScreen`].
    pub fn new(columns: usize) -> Result<Self, DisplayError> {
        if !(1..=40).contains(&columns) {
            return Err(DisplayError::BadWidth(columns));
        }
        Ok(VfdScreen {
            columns,
            column: 1,
            bytes: Vec::new(),
        })
    }

    /// Clears the display and returns the cursor to row 1, column 1 (`0x0C`).
    ///
    /// Support: Partial. The clear byte is the one the common plain-text profile uses; see
    /// [`VfdScreen`].
    pub fn clear(&mut self) -> &mut Self {
        self.bytes.push(CLEAR);
        self.column = 1;
        self
    }

    /// Moves the cursor to `row` (1 or 2) and `column` (1 up to the width).
    ///
    /// Support: Partial. The cursor command is this crate's own profile; see [`VfdScreen`].
    pub fn cursor(&mut self, row: u8, column: usize) -> Result<&mut Self, DisplayError> {
        if !(1..=2).contains(&row) || !(1..=self.columns).contains(&column) {
            return Err(DisplayError::BadCursor { row, column });
        }
        self.bytes
            .extend_from_slice(&[ESC, b'L', row, column as u8]);
        self.column = column;
        Ok(self)
    }

    /// Writes `text` at the cursor and moves the cursor past it. Text that would run past the last
    /// column is refused whole, and nothing is written.
    ///
    /// Support: Partial. Printable ASCII only; the cursor command is this crate's own profile.
    pub fn text(&mut self, text: &str) -> Result<&mut Self, DisplayError> {
        if let Some(c) = text.chars().find(|c| !(c.is_ascii_graphic() || *c == ' ')) {
            return Err(DisplayError::NotPrintable(c));
        }
        let room = self.columns + 1 - self.column;
        if text.len() > room {
            return Err(DisplayError::TooWide {
                text_columns: text.len(),
                room,
            });
        }
        self.bytes.extend_from_slice(text.as_bytes());
        self.column += text.len();
        Ok(self)
    }

    /// The bytes written so far.
    ///
    /// Support: Full.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Sends the bytes written so far to the display, as one write.
    ///
    /// Support: Partial. The cursor command is this crate's own profile; see [`VfdScreen`].
    pub fn show(&self, transport: &mut dyn Transport) -> Result<(), DisplayError> {
        transport.write(&self.bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DeviceId;
    use crate::virtual_devices::VirtualSerial;

    #[test]
    fn a_screen_is_built_byte_for_byte() {
        let mut screen = VfdScreen::new(20).expect("width");
        screen
            .clear()
            .cursor(1, 1)
            .and_then(|s| s.text("TOTAL"))
            .and_then(|s| s.cursor(2, 1))
            .and_then(|s| s.text("4.50 NZD"))
            .expect("build");
        let mut want = vec![CLEAR, ESC, b'L', 1, 1];
        want.extend_from_slice(b"TOTAL");
        want.extend_from_slice(&[ESC, b'L', 2, 1]);
        want.extend_from_slice(b"4.50 NZD");
        assert_eq!(screen.bytes(), want.as_slice());
    }

    #[test]
    fn text_follows_the_cursor_without_a_new_command() {
        let mut screen = VfdScreen::new(10).expect("width");
        screen.cursor(1, 3).expect("cursor").text("AB").expect("ab");
        screen.text("C").expect("c");
        let mut want = vec![ESC, b'L', 1, 3];
        want.extend_from_slice(b"ABC");
        assert_eq!(screen.bytes(), want.as_slice());
    }

    #[test]
    fn text_that_runs_past_the_last_column_is_refused_and_not_written() {
        let mut screen = VfdScreen::new(8).expect("width");
        screen.cursor(1, 4).expect("cursor");
        let before = screen.bytes().to_vec();
        assert_eq!(
            screen.text("ABCDEFGH").err(),
            Some(DisplayError::TooWide {
                text_columns: 8,
                room: 5
            })
        );
        assert_eq!(screen.bytes(), before.as_slice());
        // Five characters fit exactly, up to the last column.
        screen.text("ABCDE").expect("fits");
    }

    #[test]
    fn cursors_off_the_display_are_refused() {
        let mut screen = VfdScreen::new(20).expect("width");
        assert_eq!(
            screen.cursor(0, 1).err(),
            Some(DisplayError::BadCursor { row: 0, column: 1 })
        );
        assert_eq!(
            screen.cursor(3, 1).err(),
            Some(DisplayError::BadCursor { row: 3, column: 1 })
        );
        assert_eq!(
            screen.cursor(1, 21).err(),
            Some(DisplayError::BadCursor { row: 1, column: 21 })
        );
        assert!(screen.bytes().is_empty());
    }

    #[test]
    fn a_screen_refuses_non_ascii_text_and_bad_widths() {
        let mut screen = VfdScreen::new(20).expect("width");
        assert_eq!(
            screen.text("café").err(),
            Some(DisplayError::NotPrintable('é'))
        );
        assert_eq!(VfdScreen::new(0), Err(DisplayError::BadWidth(0)));
        assert_eq!(VfdScreen::new(41), Err(DisplayError::BadWidth(41)));
    }

    #[test]
    fn a_screen_is_sent_through_the_transport() {
        let id: DeviceId = "serial:COM6".parse().expect("id");
        let mut port = VirtualSerial::loopback(id);
        let mut screen = VfdScreen::new(20).expect("width");
        screen.clear().text("HELLO").expect("text");
        screen.show(&mut port).expect("show");
        assert_eq!(port.read(256).expect("read"), screen.bytes().to_vec());
    }

    #[test]
    fn a_frame_clears_then_writes_two_padded_lines() {
        let d = CustomerDisplay::new(8).expect("width");
        let bytes = d.frame("TOTAL", "4.50 NZD").expect("frame");
        assert_eq!(bytes[0], CLEAR);
        assert_eq!(&bytes[1..9], b"TOTAL   ");
        assert_eq!(&bytes[9..11], b"\r\n");
        assert_eq!(&bytes[11..], b"4.50 NZD");
    }

    #[test]
    fn long_lines_are_cut_to_the_width() {
        let d = CustomerDisplay::new(4).expect("width");
        let bytes = d.frame("ABCDEFG", "x").expect("frame");
        assert_eq!(&bytes[1..5], b"ABCD");
    }

    #[test]
    fn non_ascii_and_bad_widths_are_refused() {
        let d = CustomerDisplay::default();
        assert_eq!(d.frame("café", ""), Err(DisplayError::NotPrintable('é')));
        assert_eq!(CustomerDisplay::new(0), Err(DisplayError::BadWidth(0)));
        assert_eq!(CustomerDisplay::new(41), Err(DisplayError::BadWidth(41)));
    }

    #[test]
    fn a_frame_is_written_through_the_transport() {
        let id: DeviceId = "serial:COM5".parse().expect("id");
        let mut port = VirtualSerial::loopback(id);
        CustomerDisplay::default()
            .show(&mut port, "WELCOME", "")
            .expect("show");
        let back = port.read(256).expect("read");
        assert_eq!(back.len(), 1 + 20 + 2 + 20);
        assert_eq!(back[0], CLEAR);
    }
}
