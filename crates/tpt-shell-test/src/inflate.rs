// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A small inflate (RFC 1950 zlib, RFC 1951 deflate) for the PNG decoder.
//!
//! It handles stored, fixed and dynamic blocks, checks the zlib header and Adler-32 trailer,
//! and refuses output above a limit given by the caller, so a hostile stream cannot grow
//! without bound. It is written for clarity, not speed: screenshots are decoded once per test.

/// Length bases and extra bits for length codes 257..=285 (RFC 1951 3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Distance bases and extra bits for distance codes 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// Order in which code-length code lengths are stored (RFC 1951 3.2.7).
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

const TOO_LARGE: &str = "decompressed data is larger than the image needs";

/// Reads bits least significant first, as deflate does.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    count: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits {
            data,
            pos: 0,
            bit: 0,
            count: 0,
        }
    }

    fn bits(&mut self, n: u32) -> Result<u32, String> {
        while self.count < n {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or("deflate stream ends in the middle of a block")?;
            self.pos += 1;
            self.bit |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.count -= n;
        Ok(value)
    }

    /// Drops the bits left in the current byte.
    fn align(&mut self) {
        self.bit = 0;
        self.count = 0;
    }
}

/// A canonical Huffman code, decoded one bit at a time.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut count = [0u16; 16];
        for &len in lengths {
            count[usize::from(len)] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err("over-subscribed Huffman code".into());
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                let slot = usize::from(len);
                symbol[usize::from(offs[slot])] = sym as u16;
                offs[slot] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= bits.bits(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("invalid Huffman code".into())
    }
}

/// Inflates a zlib stream. Fails if the output would exceed `limit` bytes.
///
/// Support: Full.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    if data.len() < 6 {
        return Err("zlib stream is too short".into());
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0f != 8 || cmf >> 4 > 7 {
        return Err("zlib stream does not use deflate".into());
    }
    if (u16::from(cmf) << 8 | u16::from(flg)) % 31 != 0 {
        return Err("zlib header check failed".into());
    }
    if flg & 0x20 != 0 {
        return Err("zlib preset dictionaries are not supported".into());
    }
    let body = &data[2..data.len() - 4];
    let out = inflate(body, limit)?;
    let trailer = &data[data.len() - 4..];
    let expected = u32::from_be_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if adler32(&out) != expected {
        return Err("zlib Adler-32 check failed".into());
    }
    Ok(out)
}

/// Adler-32 checksum (RFC 1950 8.2).
///
/// Support: Full.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

/// Inflates raw deflate data. Fails if the output would exceed `limit` bytes.
///
/// Support: Full.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut bits = Bits::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bits(1)? == 1;
        match bits.bits(2)? {
            0 => stored(&mut bits, &mut out, limit)?,
            1 => {
                let (lit, dist) = fixed_codes()?;
                codes(&mut bits, &mut out, &lit, &dist, limit)?;
            }
            2 => {
                let (lit, dist) = dynamic_codes(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err("deflate block type 3 is reserved".into()),
        }
        if last {
            return Ok(out);
        }
    }
}

fn stored(bits: &mut Bits, out: &mut Vec<u8>, limit: usize) -> Result<(), String> {
    bits.align();
    let header = bits
        .data
        .get(bits.pos..bits.pos + 4)
        .ok_or("stored block header is truncated")?;
    let len = u16::from_le_bytes([header[0], header[1]]);
    let nlen = u16::from_le_bytes([header[2], header[3]]);
    if len != !nlen {
        return Err("stored block length check failed".into());
    }
    bits.pos += 4;
    let len = usize::from(len);
    if out.len() + len > limit {
        return Err(TOO_LARGE.into());
    }
    let body = bits
        .data
        .get(bits.pos..bits.pos + len)
        .ok_or("stored block is truncated")?;
    out.extend_from_slice(body);
    bits.pos += len;
    Ok(())
}

fn fixed_codes() -> Result<(Huffman, Huffman), String> {
    let mut lengths = [0u8; 288];
    for (i, len) in lengths.iter_mut().enumerate() {
        *len = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&lengths)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_codes(bits: &mut Bits) -> Result<(Huffman, Huffman), String> {
    let hlit = bits.bits(5)? as usize + 257;
    let hdist = bits.bits(5)? as usize + 1;
    let hclen = bits.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err("dynamic block declares too many codes".into());
    }
    let mut clen = [0u8; 19];
    for &slot in &CLEN_ORDER[..hclen] {
        clen[slot] = bits.bits(3)? as u8;
    }
    let clen_code = Huffman::new(&clen)?;
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0;
    while i < lengths.len() {
        let sym = clen_code.decode(bits)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths[..i]
                    .last()
                    .ok_or("length repeat with no previous length")?;
                (prev, 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if i + repeat > lengths.len() {
            return Err("code lengths overflow the block".into());
        }
        for slot in &mut lengths[i..i + repeat] {
            *slot = value;
        }
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err("dynamic block has no end-of-block code".into());
    }
    Ok((
        Huffman::new(&lengths[..hlit])?,
        Huffman::new(&lengths[hlit..])?,
    ))
}

fn codes(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), String> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => {
                if out.len() >= limit {
                    return Err(TOO_LARGE.into());
                }
                out.push(sym as u8);
            }
            256 => return Ok(()),
            257..=285 => {
                let idx = usize::from(sym - 257);
                let len = usize::from(LENGTH_BASE[idx])
                    + bits.bits(u32::from(LENGTH_EXTRA[idx]))? as usize;
                let dsym = usize::from(dist.decode(bits)?);
                if dsym >= 30 {
                    return Err("invalid distance code".into());
                }
                let d =
                    usize::from(DIST_BASE[dsym]) + bits.bits(u32::from(DIST_EXTRA[dsym]))? as usize;
                if d > out.len() {
                    return Err("distance reaches before the start of the output".into());
                }
                if out.len() + len > limit {
                    return Err(TOO_LARGE.into());
                }
                let start = out.len() - d;
                for k in 0..len {
                    let byte = out[start + k];
                    out.push(byte);
                }
            }
            _ => return Err("invalid length code".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler32_of_wikipedia_example() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn stored_block_round_trips() {
        // zlib header 78 01, one final stored block of "abc", then the Adler-32 of "abc".
        let data = [
            0x78, 0x01, 0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c', 0x02, 0x4D, 0x01, 0x27,
        ];
        assert_eq!(zlib_decompress(&data, 16).unwrap(), b"abc");
    }

    #[test]
    fn fixed_huffman_block_decodes() {
        // Raw deflate of "aaaaaaaaaa" as zlib level 9 writes it (a fixed block with a match).
        let data = [0x4b, 0x4c, 0x84, 0x01, 0x00];
        assert_eq!(inflate(&data, 64).unwrap(), b"aaaaaaaaaa");
    }

    #[test]
    fn output_over_the_limit_is_refused() {
        let data = [0x4b, 0x4c, 0x84, 0x01, 0x00];
        assert!(inflate(&data, 5).is_err());
    }

    #[test]
    fn bad_adler_checksum_is_refused() {
        let mut data = vec![0x78, 0x01, 0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        data.extend_from_slice(&[0, 0, 0, 0]);
        assert!(zlib_decompress(&data, 16).is_err());
    }

    #[test]
    fn truncated_and_reserved_blocks_are_refused() {
        assert!(inflate(&[], 16).is_err());
        // Block type 3 is reserved.
        assert!(inflate(&[0x07], 16).is_err());
        assert!(zlib_decompress(&[0x78], 16).is_err());
    }

    #[test]
    fn a_distance_before_the_output_start_is_refused() {
        // Fixed block with a length-3 match at distance 1 and no output yet. The bits, least
        // significant first: BFINAL 1, BTYPE 01, code 257 (0000001), distance 0 (00000), end.
        let data = [0x03, 0x02, 0x00];
        assert!(inflate(&data, 16).is_err());
    }
}
