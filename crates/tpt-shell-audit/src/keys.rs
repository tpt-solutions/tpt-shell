// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Keys for the audit log (spec §10.3): where the checkpoint signing key comes from, and
//! detached signatures over whole files (used by the compliance outputs).
//!
//! Three sources are supported:
//!
//! - the OS keystore, through the [`KeyStore`] trait. The Windows platform crate implements it
//!   with DPAPI. Keychain and Secret Service implementations belong to the platform crates;
//! - a key file, which holds a 32-byte seed as 64 hex digits ([`FileKeySupplier`]);
//! - an HSM or other hardware: implement [`CheckpointSigner`] so that the private key never
//!   leaves the device. Nothing hardware-specific is built into this crate.
//!
//! Every source fails closed. If no key can be found, the log is not opened.

use std::path::PathBuf;
use std::sync::Arc;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::{hex_decode, hex_encode, AuditError};

/// No key could be found or loaded. Fix: enrol a key, or point the source at a valid one.
///
/// Support: Full.
pub const CODE_KEY_UNAVAILABLE: &str = "TS0100";
/// A detached signature does not verify against the given key.
///
/// Support: Full.
pub const CODE_SIGNATURE_INVALID: &str = "TS0101";

/// Holds the checkpoint signing key. The log never sees more than a signature.
///
/// Support: Full.
pub trait CheckpointSigner: Send + Sync {
    fn verifying_key(&self) -> VerifyingKey;
    /// Signs `message` with Ed25519 and returns the 64-byte signature.
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], AuditError>;
}

impl CheckpointSigner for SigningKey {
    fn verifying_key(&self) -> VerifyingKey {
        SigningKey::verifying_key(self)
    }

    fn sign(&self, message: &[u8]) -> Result<[u8; 64], AuditError> {
        Ok(Signer::sign(self, message).to_bytes())
    }
}

/// Where the checkpoint key comes from when a log is opened.
///
/// Support: Full.
pub trait KeySupplier {
    fn signer(&self) -> Result<Arc<dyn CheckpointSigner>, AuditError>;
}

/// A small secret store, such as the Windows keystore. Errors are plain text, which is what
/// the platform stores return.
///
/// Support: Full.
pub trait KeyStore {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, name: &str, secret: &[u8]) -> Result<(), String>;
}

fn unavailable(reason: impl Into<String>) -> AuditError {
    AuditError::Refused {
        code: CODE_KEY_UNAVAILABLE,
        reason: reason.into(),
    }
}

fn seed_from_hex(text: &str) -> Option<SigningKey> {
    let bytes: [u8; 32] = hex_decode(text.trim())?.try_into().ok()?;
    Some(SigningKey::from_bytes(&bytes))
}

/// Reads the signing seed from a file of 64 hex digits.
///
/// Support: Full.
pub struct FileKeySupplier {
    path: PathBuf,
}

impl FileKeySupplier {
    /// Support: Full.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileKeySupplier { path: path.into() }
    }
}

impl KeySupplier for FileKeySupplier {
    fn signer(&self) -> Result<Arc<dyn CheckpointSigner>, AuditError> {
        let text = std::fs::read_to_string(&self.path).map_err(|e| {
            unavailable(format!(
                "cannot read the audit key file {}: {e}. Fix: create it with 64 hex digits",
                self.path.display()
            ))
        })?;
        let key = seed_from_hex(&text).ok_or_else(|| {
            unavailable(format!(
                "the audit key file {} does not hold 64 hex digits. Fix: write a 32-byte seed",
                self.path.display()
            ))
        })?;
        Ok(Arc::new(key))
    }
}

/// Reads the signing seed from a keystore entry.
///
/// Support: Full.
pub struct StoreKeySupplier<S: KeyStore> {
    store: S,
    name: String,
}

impl<S: KeyStore> StoreKeySupplier<S> {
    /// Support: Full.
    pub fn new(store: S, name: impl Into<String>) -> Self {
        StoreKeySupplier {
            store,
            name: name.into(),
        }
    }
}

impl<S: KeyStore + Send + Sync> KeySupplier for StoreKeySupplier<S> {
    fn signer(&self) -> Result<Arc<dyn CheckpointSigner>, AuditError> {
        let secret = self
            .store
            .get(&self.name)
            .map_err(|e| unavailable(format!("the keystore failed: {e}")))?;
        signer_from_secret(&self.name, secret, "the keystore")
    }
}

/// Builds a signer from the bytes a source returned for `label`. `None` means the source has no
/// key under that name, which is a failure: the log must not open unsigned.
fn signer_from_secret(
    label: &str,
    secret: Option<Vec<u8>>,
    source: &str,
) -> Result<Arc<dyn CheckpointSigner>, AuditError> {
    let secret = secret.ok_or_else(|| {
        unavailable(format!(
            "no audit key named `{label}` in {source}. Fix: enrol one before opening the log"
        ))
    })?;
    let bytes: [u8; 32] = secret.try_into().map_err(|_| {
        unavailable(format!(
            "the audit key `{label}` in {source} is not 32 bytes"
        ))
    })?;
    Ok(Arc::new(SigningKey::from_bytes(&bytes)))
}

/// Takes the key bytes from a caller-supplied closure, for sources this crate does not know
/// about: a keystore on another platform, a secret manager, or a test. The closure returns the
/// 32-byte seed for the named key, or `Ok(None)` when there is none. The audit crate never
/// depends on the platform that holds the secret.
///
/// Support: Full.
pub struct ClosureKeySupplier<F> {
    label: String,
    source: F,
}

impl<F> ClosureKeySupplier<F>
where
    F: Fn() -> Result<Option<Vec<u8>>, String>,
{
    /// `label` names the key in error messages.
    ///
    /// Support: Full. Stores the label and the closure. What the closure returns is the caller's
    /// responsibility; no test in this crate calls it.
    pub fn new(label: impl Into<String>, source: F) -> Self {
        ClosureKeySupplier {
            label: label.into(),
            source,
        }
    }
}

impl<F> KeySupplier for ClosureKeySupplier<F>
where
    F: Fn() -> Result<Option<Vec<u8>>, String> + Send + Sync,
{
    fn signer(&self) -> Result<Arc<dyn CheckpointSigner>, AuditError> {
        let secret =
            (self.source)().map_err(|e| unavailable(format!("the key source failed: {e}")))?;
        signer_from_secret(&self.label, secret, "the key source")
    }
}

/// Stores a new audit seed in the keystore and returns its public key. Refuses to replace an
/// existing key, because replacing it would break the checkpoints already written. The caller
/// supplies the seed, from the operating system's random source.
///
/// Support: Full.
pub fn enroll_key<S: KeyStore>(
    store: &S,
    name: &str,
    seed: &[u8; 32],
) -> Result<VerifyingKey, AuditError> {
    let existing = store
        .get(name)
        .map_err(|e| unavailable(format!("the keystore failed: {e}")))?;
    if existing.is_some() {
        return Err(unavailable(format!(
            "an audit key named `{name}` already exists and will not be replaced"
        )));
    }
    store
        .set(name, seed)
        .map_err(|e| unavailable(format!("the keystore refused the key: {e}")))?;
    Ok(SigningKey::from_bytes(seed).verifying_key())
}

/// The message a detached signature covers: a domain prefix and the file's SHA-256. Hashing
/// first keeps large files out of memory and keeps the signature apart from checkpoints.
fn detached_message(bytes: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("tpt-shell-detached-sha256\n{hex}").into_bytes()
}

/// Signs the bytes of a file. Returns the signature as 128 hex digits.
///
/// Support: Full.
pub fn sign_detached(signer: &dyn CheckpointSigner, bytes: &[u8]) -> Result<String, AuditError> {
    Ok(hex_encode(&signer.sign(&detached_message(bytes))?))
}

/// Checks a detached signature made by [`sign_detached`].
///
/// Support: Full.
pub fn verify_detached(
    key: &VerifyingKey,
    bytes: &[u8],
    signature_hex: &str,
) -> Result<(), AuditError> {
    let refused = |reason: &str| AuditError::Refused {
        code: CODE_SIGNATURE_INVALID,
        reason: reason.to_string(),
    };
    let raw =
        hex_decode(signature_hex.trim()).ok_or_else(|| refused("the signature is not hex"))?;
    let raw: [u8; 64] = raw
        .try_into()
        .map_err(|_| refused("the signature has the wrong length"))?;
    key.verify(&detached_message(bytes), &Signature::from_bytes(&raw))
        .map_err(|_| refused("the signature does not match this file and key"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStore(Mutex<BTreeMap<String, Vec<u8>>>);

    impl KeyStore for MemoryStore {
        fn get(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.0.lock().unwrap().get(name).cloned())
        }
        fn set(&self, name: &str, secret: &[u8]) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .insert(name.to_string(), secret.to_vec());
            Ok(())
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tpt-audit-keys-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_keystore_key_is_enrolled_once_and_then_used() {
        let store = MemoryStore::default();
        let seed = [7u8; 32];
        let public = enroll_key(&store, "audit", &seed).unwrap();
        assert!(enroll_key(&store, "audit", &[8u8; 32]).is_err());
        let signer = StoreKeySupplier::new(store, "audit").signer().unwrap();
        assert_eq!(signer.verifying_key(), public);
    }

    #[test]
    fn a_missing_keystore_key_fails_closed_with_a_code() {
        let err = StoreKeySupplier::new(MemoryStore::default(), "audit")
            .signer()
            .err()
            .unwrap();
        assert!(err.to_string().starts_with(CODE_KEY_UNAVAILABLE));
    }

    #[test]
    fn a_key_file_must_hold_a_32_byte_seed() {
        let dir = temp_dir("file");
        let good = dir.join("good.key");
        std::fs::write(&good, format!("{}\n", "2a".repeat(32))).unwrap();
        let signer = FileKeySupplier::new(&good).signer().unwrap();
        assert_eq!(
            signer.verifying_key(),
            SigningKey::from_bytes(&[42; 32]).verifying_key()
        );

        let short = dir.join("short.key");
        std::fs::write(&short, "2a2a").unwrap();
        assert!(FileKeySupplier::new(&short).signer().is_err());
        assert!(FileKeySupplier::new(dir.join("absent.key"))
            .signer()
            .is_err());
    }

    #[test]
    fn detached_signatures_verify_and_catch_changes() {
        let key = SigningKey::from_bytes(&[3; 32]);
        let signature = sign_detached(&key, b"artifact bytes").unwrap();
        assert_eq!(signature.len(), 128);
        assert!(verify_detached(&key.verifying_key(), b"artifact bytes", &signature).is_ok());
        assert!(verify_detached(&key.verifying_key(), b"artifact byte5", &signature).is_err());
        let other = SigningKey::from_bytes(&[4; 32]);
        assert!(verify_detached(&other.verifying_key(), b"artifact bytes", &signature).is_err());
        assert!(verify_detached(&key.verifying_key(), b"x", "zz").is_err());
    }
}
