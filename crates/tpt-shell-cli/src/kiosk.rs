// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell kiosk sign <config> --key <key-file>`: writes `<config>.sig`, a detached Ed25519
//! signature over the exact bytes of the config. The kiosk loads a config only when the
//! signature checks out (see `tpt_shell_kiosk::config`). Nothing is signed unless this command
//! runs with a key file.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tpt_shell_kiosk::config;

/// The signature file for `config`: the same name with `.sig` added.
///
/// Support: Full.
pub fn signature_path(config: &Path) -> PathBuf {
    let mut name: OsString = config.as_os_str().to_owned();
    name.push(".sig");
    PathBuf::from(name)
}

/// Reads a key file of 64 hex digits (a 32-byte secret seed).
fn secret_from_hex(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// Signs `config` with the key in `key`, writes `<config>.sig`, and checks the signature with the
/// public key before reporting success. The config must be JSON, because the kiosk parses it.
///
/// Support: Full.
pub fn sign(config_path: &Path, key_path: &Path) -> Result<ExitCode, String> {
    let key_text = fs::read_to_string(key_path)
        .map_err(|e| format!("cannot read {}: {e}", key_path.display()))?;
    let secret = secret_from_hex(&key_text).ok_or_else(|| {
        format!(
            "{} is not a 64-digit hex key (a 32-byte seed)",
            key_path.display()
        )
    })?;
    let body =
        fs::read(config_path).map_err(|e| format!("cannot read {}: {e}", config_path.display()))?;
    serde_json::from_slice::<serde_json::Value>(&body).map_err(|e| {
        format!(
            "{} is not JSON, so it was not signed: {e}",
            config_path.display()
        )
    })?;

    let signature = config::sign(&body, &secret);
    let public = config::public_key_hex(&secret);
    config::verify(&body, &signature, &public)
        .map_err(|e| format!("the signature does not verify against its own key: {e}"))?;

    let sig_path = signature_path(config_path);
    fs::write(&sig_path, format!("{signature}\n"))
        .map_err(|e| format!("cannot write {}: {e}", sig_path.display()))?;
    println!("signed {}", config_path.display());
    println!("signature: {}", sig_path.display());
    println!("public key: {public}");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("tpt-cli-kiosk-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

    #[test]
    fn the_signature_file_sits_beside_the_config() {
        assert_eq!(
            signature_path(Path::new("kiosk.json")),
            PathBuf::from("kiosk.json.sig")
        );
    }

    #[test]
    fn a_signed_config_verifies_against_the_printed_key() {
        let dir = scratch("ok");
        let config_path = dir.join("kiosk.json");
        let body = br#"{"start":"index.html"}"#;
        fs::write(&config_path, body).unwrap();
        let key_path = dir.join("kiosk.key");
        fs::write(&key_path, format!("{KEY}\n")).unwrap();

        assert_eq!(sign(&config_path, &key_path).unwrap(), ExitCode::SUCCESS);
        let signature = fs::read_to_string(signature_path(&config_path)).unwrap();
        let secret = secret_from_hex(KEY).unwrap();
        let public = config::public_key_hex(&secret);
        assert!(config::verify(body, signature.trim(), &public).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_key_is_refused_and_nothing_is_written() {
        let dir = scratch("badkey");
        let config_path = dir.join("kiosk.json");
        fs::write(&config_path, b"{}").unwrap();
        let key_path = dir.join("kiosk.key");
        fs::write(&key_path, "not hex").unwrap();
        assert!(sign(&config_path, &key_path).is_err());
        assert!(!signature_path(&config_path).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_key_or_config_is_an_error() {
        let dir = scratch("missing");
        let config_path = dir.join("kiosk.json");
        fs::write(&config_path, b"{}").unwrap();
        assert!(sign(&config_path, &dir.join("no-such.key")).is_err());
        let key_path = dir.join("kiosk.key");
        fs::write(&key_path, KEY).unwrap();
        assert!(sign(&dir.join("no-such.json"), &key_path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_config_that_is_not_json_is_not_signed() {
        let dir = scratch("notjson");
        let config_path = dir.join("kiosk.json");
        fs::write(&config_path, b"not json").unwrap();
        let key_path = dir.join("kiosk.key");
        fs::write(&key_path, KEY).unwrap();
        assert!(sign(&config_path, &key_path).is_err());
        assert!(!signature_path(&config_path).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn secret_keys_must_be_64_hex_digits() {
        assert!(secret_from_hex(KEY).is_some());
        assert!(secret_from_hex(&KEY[..63]).is_none());
        assert!(secret_from_hex(&format!("+{}", &KEY[..63])).is_none());
    }
}
