// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Base64 (RFC 4648, standard alphabet, with padding) for asset bytes in a recording. Written
//! here so the format needs no dependency.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes bytes as base64 text.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
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

/// Decodes base64 text. Refuses bad characters, bad padding and a length that is not a
/// multiple of four. Decoding stops with an error once the output would exceed `limit`.
pub fn decode(text: &str, limit: usize) -> Result<Vec<u8>, String> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("base64 length is not a multiple of four".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let groups = bytes.len() / 4;
    for (g, quad) in bytes.chunks(4).enumerate() {
        let last = g + 1 == groups;
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return Err("base64 padding is in the wrong place".into());
        }
        let mut n: u32 = 0;
        for &c in &quad[..4 - pad] {
            n = (n << 6) | u32::from(value(c)?);
        }
        n <<= 6 * pad as u32;
        let raw = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let take = 3 - pad;
        if out.len() + take > limit {
            return Err("base64 data is larger than the limit".into());
        }
        out.extend_from_slice(&raw[..take]);
    }
    Ok(out)
}

fn value(c: u8) -> Result<u8, String> {
    match c {
        b'A'..=b'Z' => Ok(c - b'A'),
        b'a'..=b'z' => Ok(c - b'a' + 26),
        b'0'..=b'9' => Ok(c - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err("base64 contains a character outside the alphabet".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_4648_vectors_round_trip() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (raw, text) in cases {
            assert_eq!(encode(raw.as_bytes()), text);
            assert_eq!(decode(text, 64).unwrap(), raw.as_bytes());
        }
    }

    #[test]
    fn every_byte_value_round_trips() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&bytes), 256).unwrap(), bytes);
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(decode("Zm9", 16).is_err(), "length");
        assert!(decode("Zm!v", 16).is_err(), "alphabet");
        assert!(decode("Zg==Zg==", 16).is_err(), "padding in the middle");
        assert!(decode("Z===", 16).is_err(), "too much padding");
        assert!(decode("Zm9v", 2).is_err(), "limit");
    }
}
