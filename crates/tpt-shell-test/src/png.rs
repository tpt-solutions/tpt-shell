// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A PNG decoder for screenshots (spec §10.6). It turns a file into the RGBA [`Image`] that
//! the visual comparer reads.
//!
//! Support: Full for non-interlaced images of every colour type and bit depth in the PNG
//! specification. Partial: Adam7 interlaced files are refused, and `tRNS` colour keys on
//! grey and RGB images are ignored (only the palette `tRNS` table is applied). Ancillary
//! chunks are skipped. Every chunk CRC is checked, and the decoder refuses images above
//! [`MAX_PIXELS`] so a hostile header cannot exhaust memory.

use crate::inflate::zlib_decompress;
use crate::visual::Image;

/// The largest image accepted, in pixels (about 160 MB as RGBA).
///
/// Support: Full.
pub const MAX_PIXELS: u64 = 40_000_000;

const SIGNATURE: [u8; 8] = *b"\x89PNG\r\n\x1a\n";

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

/// CRC-32 as PNG uses it (ISO 3309).
///
/// Support: Full.
pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for part in parts {
        for &byte in *part {
            c = CRC_TABLE[((c ^ u32::from(byte)) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xFFFF_FFFF
}

/// Decodes a PNG file into RGBA pixels. Errors name the problem and never panic.
///
/// Support: Partial. Adam7 interlaced files are refused, and tRNS colour keys on grey and RGB
/// images are ignored (only the palette tRNS table is applied).
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err("not a PNG file".into());
    }
    let mut pos = 8;
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_end = false;
    while !seen_end {
        let fixed = bytes
            .get(pos..pos + 8)
            .ok_or("PNG ends before its IEND chunk")?;
        let len = u32::from_be_bytes([fixed[0], fixed[1], fixed[2], fixed[3]]) as usize;
        let kind = [fixed[4], fixed[5], fixed[6], fixed[7]];
        let body_start = pos + 8;
        let body_end = body_start
            .checked_add(len)
            .ok_or("PNG chunk length overflows")?;
        let body = bytes
            .get(body_start..body_end)
            .ok_or("PNG chunk runs past the end of the file")?;
        let crc_bytes = bytes
            .get(body_end..body_end + 4)
            .ok_or("PNG chunk has no CRC")?;
        let stored_crc =
            u32::from_be_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        if crc32(&[&kind, body]) != stored_crc {
            return Err(format!(
                "CRC check failed on the {} chunk",
                chunk_name(&kind)
            ));
        }
        if header.is_none() && &kind != b"IHDR" {
            return Err("PNG does not start with an IHDR chunk".into());
        }
        match &kind {
            b"IHDR" => {
                if header.is_some() {
                    return Err("PNG has two IHDR chunks".into());
                }
                header = Some(parse_header(body)?);
            }
            b"PLTE" => {
                if body.len() % 3 != 0 || body.is_empty() || body.len() > 3 * 256 {
                    return Err("PLTE chunk has a bad length".into());
                }
                palette = body.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => seen_end = true,
            _ => {}
        }
        pos = body_end + 4;
    }
    let header = header.ok_or("PNG has no IHDR chunk")?;
    if header.color == 3 && palette.is_empty() {
        return Err("palette PNG has no PLTE chunk".into());
    }
    if idat.is_empty() {
        return Err("PNG has no image data".into());
    }
    let stride = header.stride();
    let raw_len = header.height as usize * (stride + 1);
    let raw = zlib_decompress(&idat, raw_len)?;
    if raw.len() != raw_len {
        return Err("PNG image data is shorter than its header says".into());
    }
    let pixels = unfilter(
        &raw,
        stride,
        header.bytes_per_pixel(),
        header.height as usize,
    )?;
    let rgba = to_rgba(&header, &pixels, stride, &palette, &trns)?;
    Image::new(header.width, header.height, rgba)
}

fn chunk_name(kind: &[u8; 4]) -> String {
    kind.iter()
        .map(|&b| if b.is_ascii_graphic() { b as char } else { '?' })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }

    fn stride(&self) -> usize {
        (self.width as usize * self.channels() * usize::from(self.depth)).div_ceil(8)
    }

    /// The filter distance in bytes: the bytes of one whole pixel, at least one.
    fn bytes_per_pixel(&self) -> usize {
        (self.channels() * usize::from(self.depth)).div_ceil(8)
    }
}

fn parse_header(body: &[u8]) -> Result<Header, String> {
    if body.len() != 13 {
        return Err("IHDR chunk has the wrong length".into());
    }
    let width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]);
    let height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
    let (depth, color) = (body[8], body[9]);
    if width == 0 || height == 0 {
        return Err("PNG has a zero width or height".into());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "PNG is {width}x{height}, above the {MAX_PIXELS}-pixel limit"
        ));
    }
    let depth_ok = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(depth, 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => return Err(format!("PNG colour type {color} does not exist")),
    };
    if !depth_ok {
        return Err(format!(
            "PNG colour type {color} cannot have bit depth {depth}"
        ));
    }
    if body[10] != 0 || body[11] != 0 {
        return Err("PNG uses an unknown compression or filter method".into());
    }
    if body[12] != 0 {
        return Err("interlaced (Adam7) PNG is not supported".into());
    }
    Ok(Header {
        width,
        height,
        depth,
        color,
    })
}

/// Reverses the per-row filters. Each row starts with its filter type byte.
fn unfilter(raw: &[u8], stride: usize, bpp: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        let (before, rest) = out.split_at_mut(y * stride);
        let prev: &[u8] = if y == 0 {
            &[]
        } else {
            &before[(y - 1) * stride..]
        };
        let row = &mut rest[..stride];
        for i in 0..stride {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = if y > 0 { prev[i] } else { 0 };
            let c = if y > 0 && i >= bpp { prev[i - bpp] } else { 0 };
            let x = src[i];
            row[i] = match filter {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                _ => return Err(format!("PNG row {y} has unknown filter type {filter}")),
            };
        }
    }
    Ok(out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// The sample `n` of a row, as stored (not scaled).
fn sample(row: &[u8], n: usize, depth: u8) -> u16 {
    match depth {
        16 => u16::from_be_bytes([row[2 * n], row[2 * n + 1]]),
        8 => u16::from(row[n]),
        d => {
            let bit = n * usize::from(d);
            let shift = 8 - usize::from(d) - bit % 8;
            let mask = (1u16 << d) - 1;
            (u16::from(row[bit / 8]) >> shift) & mask
        }
    }
}

/// Scales a sample of `depth` bits to 0..=255.
fn to_8_bit(value: u16, depth: u8) -> u8 {
    match depth {
        16 => (value >> 8) as u8,
        8 => value as u8,
        d => (value * 255 / ((1u16 << d) - 1)) as u8,
    }
}

fn to_rgba(
    header: &Header,
    pixels: &[u8],
    stride: usize,
    palette: &[[u8; 3]],
    trns: &[u8],
) -> Result<Vec<u8>, String> {
    let width = header.width as usize;
    let channels = header.channels();
    let depth = header.depth;
    let mut out = Vec::with_capacity(width * header.height as usize * 4);
    for row in pixels.chunks(stride) {
        for x in 0..width {
            let at = |c: usize| sample(row, x * channels + c, depth);
            let px = match header.color {
                0 => {
                    let g = to_8_bit(at(0), depth);
                    [g, g, g, 255]
                }
                2 => [
                    to_8_bit(at(0), depth),
                    to_8_bit(at(1), depth),
                    to_8_bit(at(2), depth),
                    255,
                ],
                3 => {
                    let index = usize::from(at(0));
                    let [r, g, b] = *palette
                        .get(index)
                        .ok_or("PNG pixel uses a palette index that is not in PLTE")?;
                    let alpha = trns.get(index).copied().unwrap_or(255);
                    [r, g, b, alpha]
                }
                4 => {
                    let g = to_8_bit(at(0), depth);
                    [g, g, g, to_8_bit(at(1), depth)]
                }
                _ => [
                    to_8_bit(at(0), depth),
                    to_8_bit(at(1), depth),
                    to_8_bit(at(2), depth),
                    to_8_bit(at(3), depth),
                ],
            };
            out.extend_from_slice(&px);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRADIENT: &[u8] = include_bytes!("../tests/data/gradient_rgb.png");
    const STORED: &[u8] = include_bytes!("../tests/data/gradient_stored.png");
    const PALETTE: &[u8] = include_bytes!("../tests/data/palette_depth2.png");
    const GREY_ALPHA: &[u8] = include_bytes!("../tests/data/grey_alpha.png");

    /// The pixel the fixtures encode at (x, y) for the RGB gradient.
    fn gradient_px(x: u32, y: u32) -> [u8; 4] {
        [
            ((x * 4) & 255) as u8,
            ((y * 5) & 255) as u8,
            (((x + y) * 3) & 255) as u8,
            255,
        ]
    }

    fn pixel(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width() + x) * 4) as usize;
        let rgba = img.rgba();
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    #[test]
    fn dynamic_huffman_gradient_with_all_filters_decodes() {
        let img = decode(GRADIENT).unwrap();
        assert_eq!((img.width(), img.height()), (64, 48));
        for &(x, y) in &[
            (0, 0),
            (1, 0),
            (63, 47),
            (10, 20),
            (33, 1),
            (5, 4),
            (60, 45),
        ] {
            assert_eq!(pixel(&img, x, y), gradient_px(x, y), "pixel {x},{y}");
        }
    }

    #[test]
    fn stored_blocks_decode_to_the_same_pixels() {
        let a = decode(GRADIENT).unwrap();
        let b = decode(STORED).unwrap();
        assert_eq!(a.rgba(), b.rgba());
    }

    #[test]
    fn palette_at_depth_two_applies_trns() {
        let img = decode(PALETTE).unwrap();
        // Index is (x + y) % 4; palette is red, green, blue, white; alpha is 255,128,0,255.
        let colours = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        let alphas = [255u8, 128, 0, 255];
        for y in 0..4u32 {
            for x in 0..4u32 {
                let idx = ((x + y) % 4) as usize;
                let [r, g, b] = colours[idx];
                assert_eq!(pixel(&img, x, y), [r, g, b, alphas[idx]], "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn grey_with_alpha_decodes() {
        let img = decode(GREY_ALPHA).unwrap();
        assert_eq!(pixel(&img, 0, 0), [10, 10, 10, 200]);
        assert_eq!(pixel(&img, 2, 1), [60, 60, 60, 32]);
    }

    #[test]
    fn a_corrupted_chunk_fails_its_crc() {
        let mut bytes = GRADIENT.to_vec();
        let at = bytes.len() / 2;
        bytes[at] ^= 0x55;
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn truncated_files_are_refused_at_every_length() {
        for len in 0..GREY_ALPHA.len() {
            assert!(decode(&GREY_ALPHA[..len]).is_err(), "length {len}");
        }
    }

    #[test]
    fn not_a_png_is_refused() {
        assert!(decode(b"GIF89a").is_err());
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn crc_of_iend_is_the_known_value() {
        assert_eq!(crc32(&[b"IEND"]), 0xAE42_6082);
    }

    #[test]
    fn a_header_above_the_pixel_limit_is_refused_before_any_allocation() {
        let mut bytes = GREY_ALPHA.to_vec();
        // Width 100000 and height 100000 in IHDR, with a matching CRC.
        let ihdr = [0, 1, 0x86, 0xA0, 0, 1, 0x86, 0xA0, 8, 4, 0, 0, 0];
        let crc = crc32(&[b"IHDR", &ihdr]).to_be_bytes();
        bytes[16..29].copy_from_slice(&ihdr);
        bytes[29..33].copy_from_slice(&crc);
        let err = decode(&bytes).unwrap_err();
        assert!(err.contains("limit"), "{err}");
    }

    #[test]
    fn unknown_filter_type_is_refused() {
        let raw = [9u8, 1, 2, 3];
        assert!(unfilter(&raw, 3, 1, 1).is_err());
    }

    #[test]
    fn paeth_matches_the_specification_example() {
        assert_eq!(paeth(10, 20, 10), 20);
        assert_eq!(paeth(0, 0, 0), 0);
    }
}
