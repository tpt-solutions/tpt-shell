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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::DeviceId;
    use crate::virtual_devices::VirtualSerial;

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
