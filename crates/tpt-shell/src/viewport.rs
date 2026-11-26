// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Frames for a page: a rendered RGBA8 image sent to a `<canvas>` as an event.
//!
//! A 3D engine (or any renderer) produces row-major RGBA8 pixels. The host sends them
//! with `UiHandle::viewport_frame`. The page receives the event `viewport.frame` and draws
//! it with `ImageData`. The payload is JSON: `{ width, height, format: "rgba8", data }`,
//! where `data` is the pixel bytes in standard base64.
//!
//! Events are not queued, so a page that is still loading misses early frames. Send frames
//! from the render loop, and the next one replaces any that were missed.

use serde_json::{json, Value};
use tpt_shell_core::{Error, Result};

/// The event name a page listens for.
pub const FRAME_EVENT: &str = "viewport.frame";

/// The format name in the payload. The only format for now.
pub const FRAME_FORMAT: &str = "rgba8";

/// Builds the event data for one frame. Fails if the pixel buffer is not exactly
/// `width * height * 4` bytes, or the frame is empty.
pub fn frame_data(width: u32, height: u32, rgba: &[u8]) -> Result<Value> {
    if width == 0 || height == 0 {
        return Err(Error::Backend(
            "viewport frame must not be empty".to_string(),
        ));
    }
    let expected = u64::from(width) * u64::from(height) * 4;
    if rgba.len() as u64 != expected {
        return Err(Error::Backend(format!(
            "viewport frame is {}x{}: expected {expected} bytes, got {}",
            width,
            height,
            rgba.len()
        )));
    }
    Ok(json!({
        "width": width,
        "height": height,
        "format": FRAME_FORMAT,
        "data": base64_encode(rgba),
    }))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (RFC 4648) with `=` padding. Written here to keep the shell free of an
/// encoding dependency.
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_frame_carries_its_size_format_and_pixels() {
        let data = frame_data(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        assert_eq!(data["width"], 2);
        assert_eq!(data["height"], 1);
        assert_eq!(data["format"], FRAME_FORMAT);
        // Base64 of the eight pixel bytes FF 00 00 FF 00 00 FF FF.
        assert_eq!(data["data"], "/wAA/wAA//8=");
    }

    #[test]
    fn a_buffer_of_the_wrong_length_is_refused() {
        assert!(frame_data(2, 2, &[0; 15]).is_err());
        assert!(frame_data(2, 2, &[0; 17]).is_err());
        assert!(frame_data(2, 2, &[0; 16]).is_ok());
    }

    #[test]
    fn an_empty_frame_is_refused() {
        assert!(frame_data(0, 4, &[]).is_err());
        assert!(frame_data(4, 0, &[]).is_err());
    }
}
