// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Encrypted profile export (spec §10.7). A profile folder becomes one file that only the
//! passphrase opens.
//!
//! Format (all integers big-endian):
//!
//! - magic `TPTPROF1` (8 bytes)
//! - PBKDF2-HMAC-SHA256 iteration count (4 bytes)
//! - salt (16 bytes), random per export
//! - nonce (12 bytes), random per export
//! - ChaCha20-Poly1305 ciphertext and tag over the payload. The header is authenticated as
//!   associated data, so changing the iteration count or the salt fails the check.
//!
//! The payload is a list of entries: path length (u32), relative path with `/` separators,
//! file length (u64), file bytes. Symbolic links are skipped. The file is written under a
//! temporary name and renamed into place, so a failed export leaves no partial file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hmac::Hmac;
use sha2::Sha256;

const MAGIC: &[u8; 8] = b"TPTPROF1";
const ITERATIONS: u32 = 600_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + 4 + SALT_LEN + NONCE_LEN;
/// Shortest passphrase accepted. Shorter ones are refused, so a weak export is not written.
///
/// Support: Full.
pub const MIN_PASSPHRASE: usize = 8;

/// One file in an export: its path under the profile and its bytes.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// Writes the files under `dir` to `out`, encrypted with `passphrase`.
///
/// Support: Full.
pub fn export_dir(dir: &Path, out: &Path, passphrase: &str) -> io::Result<()> {
    export_with_iterations(dir, out, passphrase, ITERATIONS)
}

fn export_with_iterations(
    dir: &Path,
    out: &Path,
    passphrase: &str,
    iterations: u32,
) -> io::Result<()> {
    check_passphrase(passphrase)?;
    let mut payload = Vec::new();
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    for file in &files {
        let path = file.path.as_bytes();
        let len = u32::try_from(path.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "path too long"))?;
        payload.extend_from_slice(&len.to_be_bytes());
        payload.extend_from_slice(path);
        payload.extend_from_slice(&(file.bytes.len() as u64).to_be_bytes());
        payload.extend_from_slice(&file.bytes);
    }

    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    random(&mut salt)?;
    random(&mut nonce)?;

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&iterations.to_be_bytes());
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);

    let key = derive_key(passphrase, &salt, iterations);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &payload,
                aad: &header,
            },
        )
        .map_err(|_| io::Error::other("encryption failed"))?;

    let mut bytes = header;
    bytes.extend_from_slice(&sealed);
    write_atomically(out, &bytes)
}

/// Opens an export written by [`export_dir`]. A wrong passphrase or a changed file is an
/// `InvalidData` error, and no partial result is returned.
///
/// Support: Full.
pub fn read_export(file: &Path, passphrase: &str) -> io::Result<Vec<ExportedFile>> {
    let bytes = fs::read(file)?;
    if bytes.len() < HEADER_LEN + 16 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(invalid("not a tpt-shell profile export"));
    }
    let header = &bytes[..HEADER_LEN];
    let iterations = u32::from_be_bytes(bytes[8..12].try_into().expect("4 bytes"));
    if iterations == 0 {
        return Err(invalid("the export has a zero iteration count"));
    }
    let salt = &bytes[12..12 + SALT_LEN];
    let nonce = &bytes[12 + SALT_LEN..HEADER_LEN];

    let key = derive_key(passphrase, salt, iterations);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let payload = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: &bytes[HEADER_LEN..],
                aad: header,
            },
        )
        .map_err(|_| invalid("the passphrase is wrong or the export was changed"))?;
    parse_payload(&payload)
}

fn parse_payload(mut rest: &[u8]) -> io::Result<Vec<ExportedFile>> {
    let mut files = Vec::new();
    while !rest.is_empty() {
        let len = take(&mut rest, 4)?;
        let len = u32::from_be_bytes(len.try_into().expect("4 bytes")) as usize;
        let path = std::str::from_utf8(take(&mut rest, len)?)
            .map_err(|_| invalid("a path in the export is not UTF-8"))?
            .to_string();
        let size = take(&mut rest, 8)?;
        let size = u64::from_be_bytes(size.try_into().expect("8 bytes"));
        let size = usize::try_from(size).map_err(|_| invalid("a file is too large"))?;
        let bytes = take(&mut rest, size)?.to_vec();
        files.push(ExportedFile { path, bytes });
    }
    Ok(files)
}

fn take<'a>(rest: &mut &'a [u8], n: usize) -> io::Result<&'a [u8]> {
    if rest.len() < n {
        return Err(invalid("the export ends in the middle of an entry"));
    }
    let (head, tail) = rest.split_at(n);
    *rest = tail;
    Ok(head)
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<ExportedFile>) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<io::Result<_>>()?;
    entries.sort();
    for path in entries {
        let kind = fs::symlink_metadata(&path)?.file_type();
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|e| io::Error::other(e.to_string()))?;
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push(ExportedFile {
                path: name,
                bytes: fs::read(&path)?,
            });
        }
    }
    Ok(())
}

fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), salt, iterations, &mut key)
        .expect("HMAC accepts keys of any length");
    key
}

fn check_passphrase(passphrase: &str) -> io::Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the passphrase must be at least {MIN_PASSPHRASE} characters"),
        ));
    }
    Ok(())
}

fn random(buf: &mut [u8]) -> io::Result<()> {
    getrandom::getrandom(buf).map_err(|e| io::Error::other(format!("no random source: {e}")))
}

fn write_atomically(out: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut name = out.as_os_str().to_owned();
    name.push(".tmp");
    let tmp = PathBuf::from(name);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, out).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tpt-profile-export-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_export_opens_with_the_passphrase_and_keeps_every_file() {
        let root = scratch("roundtrip");
        let profile = root.join("profile");
        fs::create_dir_all(profile.join("deep")).unwrap();
        fs::write(profile.join("consent.txt"), "a=allow").unwrap();
        fs::write(profile.join("deep/data.bin"), vec![1u8, 2, 3]).unwrap();
        let out = root.join("work.tptprof");

        export_with_iterations(&profile, &out, "correct horse", 1000).unwrap();
        let files = read_export(&out, "correct horse").unwrap();
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|f| f.path == "consent.txt" && f.bytes == b"a=allow"));
        assert!(files
            .iter()
            .any(|f| f.path == "deep/data.bin" && f.bytes == [1, 2, 3]));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_plaintext_does_not_appear_in_the_export() {
        let root = scratch("plain");
        let profile = root.join("profile");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("secret.txt"), "very-distinctive-marker").unwrap();
        let out = root.join("out.tptprof");
        export_with_iterations(&profile, &out, "correct horse", 1000).unwrap();
        let raw = fs::read(&out).unwrap();
        assert!(!raw
            .windows(b"very-distinctive-marker".len())
            .any(|w| w == b"very-distinctive-marker"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wrong_passphrase_or_a_changed_byte_is_refused() {
        let root = scratch("tamper");
        let profile = root.join("profile");
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join("a.txt"), "hello").unwrap();
        let out = root.join("out.tptprof");
        export_with_iterations(&profile, &out, "correct horse", 1000).unwrap();

        assert_eq!(
            read_export(&out, "wrong passphrase").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let mut raw = fs::read(&out).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        fs::write(&out, &raw).unwrap();
        assert!(read_export(&out, "correct horse").is_err());

        // Changing the iteration count is caught by the associated data check.
        let mut raw = fs::read(&out).unwrap();
        raw[last] ^= 1;
        raw[11] = raw[11].wrapping_add(1);
        fs::write(&out, &raw).unwrap();
        assert!(read_export(&out, "correct horse").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn short_passphrases_and_non_exports_are_refused() {
        let root = scratch("refuse");
        let profile = root.join("profile");
        fs::create_dir_all(&profile).unwrap();
        let out = root.join("out.tptprof");
        assert_eq!(
            export_dir(&profile, &out, "short").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!out.exists(), "a refused export writes nothing");
        fs::write(&out, "plain text, not an export at all").unwrap();
        assert_eq!(
            read_export(&out, "correct horse").unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_dir_all(&root);
    }
}
