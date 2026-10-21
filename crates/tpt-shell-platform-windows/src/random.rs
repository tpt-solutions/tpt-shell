// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Operating-system random bytes (spec §9). Blob ids are capabilities, so they come from the
//! system generator and never from a generator that a page or an attacker could predict.

use std::ptr::null_mut;

use windows_sys::Win32::Security::Cryptography::{
    BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
};

/// Fills `buf` with random bytes from the system.
pub fn fill(buf: &mut [u8]) -> Result<(), String> {
    let len = u32::try_from(buf.len()).map_err(|_| "random buffer is too large".to_string())?;
    // SAFETY: `buf` is valid for `len` bytes. A null algorithm handle with the system preferred
    // generator is the documented way to call BCryptGenRandom without opening a provider.
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            buf.as_mut_ptr(),
            len,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        Err(format!("BCryptGenRandom failed: status {status:#x}"))
    } else {
        Ok(())
    }
}

/// A random 128-bit id as 32 lowercase hex digits, the form blob ids take.
pub fn hex_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_32_lowercase_hex_digits_and_differ() {
        let a = hex_id().expect("first id");
        let b = hex_id().expect("second id");
        assert_eq!(a.len(), 32);
        assert!(a
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
        assert_ne!(a, b);
    }
}
