// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Signed local kiosk config (spec §10.1). The config is a JSON file, and next to it is a
//! detached Ed25519 signature over the exact bytes of that file. The kiosk uses the config only
//! if the signature checks out against the public key it was built or installed with, so a
//! config edited on the device is not applied.

use std::fmt;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// Support: Full.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The public key or the signature is not the right hex length.
    Encoding(&'static str),
    /// The signature does not match the config bytes under this key.
    BadSignature,
    /// The config file is valid, but is not JSON.
    NotJson(String),
    /// A file could not be read.
    Io(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Encoding(what) => write!(f, "{what} is not valid hex of the right length"),
            ConfigError::BadSignature => write!(f, "the kiosk config signature does not verify"),
            ConfigError::NotJson(err) => write!(f, "the kiosk config is not JSON: {err}"),
            ConfigError::Io(err) => write!(f, "cannot read the kiosk config: {err}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Checks `body` against the hex `signature` under the hex `public_key`.
///
/// Support: Full.
pub fn verify(body: &[u8], signature: &str, public_key: &str) -> Result<(), ConfigError> {
    let key_bytes: [u8; 32] = decode_hex(public_key, "the public key")?;
    let key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| ConfigError::Encoding("the public key"))?;
    let sig_bytes: [u8; 64] = decode_hex(signature, "the signature")?;
    let sig = Signature::from_bytes(&sig_bytes);
    key.verify(body, &sig)
        .map_err(|_| ConfigError::BadSignature)
}

/// Reads the config and its signature from disk, verifies them, and parses the config as JSON.
///
/// Support: Full.
pub fn load(
    config: &Path,
    signature: &Path,
    public_key: &str,
) -> Result<serde_json::Value, ConfigError> {
    let body = std::fs::read(config).map_err(|e| ConfigError::Io(e.to_string()))?;
    let signature =
        std::fs::read_to_string(signature).map_err(|e| ConfigError::Io(e.to_string()))?;
    verify(&body, signature.trim(), public_key)?;
    serde_json::from_slice(&body).map_err(|e| ConfigError::NotJson(e.to_string()))
}

/// Signs `body` with a 32-byte secret key and returns the signature as hex. Used by the tool
/// that writes configs, and by the tests.
///
/// Support: Full.
pub fn sign(body: &[u8], secret_key: &[u8; 32]) -> String {
    let key = SigningKey::from_bytes(secret_key);
    encode_hex(&key.sign(body).to_bytes())
}

/// The public key for a 32-byte secret key, as hex.
///
/// Support: Full.
pub fn public_key_hex(secret_key: &[u8; 32]) -> String {
    encode_hex(
        &SigningKey::from_bytes(secret_key)
            .verifying_key()
            .to_bytes(),
    )
}

fn decode_hex<const N: usize>(text: &str, what: &'static str) -> Result<[u8; N], ConfigError> {
    let text = text.trim();
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConfigError::Encoding(what));
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| ConfigError::Encoding(what))?;
    }
    Ok(out)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [7; 32];
    const BODY: &[u8] = br#"{"mode":"single-app","home":"app://index.html"}"#;

    #[test]
    fn a_signed_config_verifies_and_parses() {
        let public = public_key_hex(&SECRET);
        let sig = sign(BODY, &SECRET);
        assert!(verify(BODY, &sig, &public).is_ok());
    }

    #[test]
    fn an_edited_config_is_refused() {
        let public = public_key_hex(&SECRET);
        let sig = sign(BODY, &SECRET);
        let edited = br#"{"mode":"single-app","home":"https://evil.example/"}"#;
        assert_eq!(
            verify(edited, &sig, &public),
            Err(ConfigError::BadSignature)
        );
    }

    #[test]
    fn a_config_signed_by_another_key_is_refused() {
        let other = [9u8; 32];
        let sig = sign(BODY, &other);
        assert_eq!(
            verify(BODY, &sig, &public_key_hex(&SECRET)),
            Err(ConfigError::BadSignature)
        );
    }

    #[test]
    fn malformed_hex_is_refused_with_the_field_named() {
        let public = public_key_hex(&SECRET);
        assert_eq!(
            verify(BODY, "not-hex", &public),
            Err(ConfigError::Encoding("the signature"))
        );
        assert_eq!(
            verify(BODY, &sign(BODY, &SECRET), "abcd"),
            Err(ConfigError::Encoding("the public key"))
        );
    }

    #[test]
    fn load_reads_the_pair_from_disk() {
        let dir = std::env::temp_dir().join(format!("tpt-kiosk-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let config = dir.join("kiosk.json");
        let signature = dir.join("kiosk.json.sig");
        std::fs::write(&config, BODY).expect("write config");
        std::fs::write(&signature, sign(BODY, &SECRET)).expect("write signature");
        let value = load(&config, &signature, &public_key_hex(&SECRET)).expect("load");
        assert_eq!(value["mode"], "single-app");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
