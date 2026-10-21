// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Append-only audit log with a SHA-256 hash chain (spec §10.3, basic chain).
//!
//! One line of the file is one record: canonical JSON with sorted keys and no whitespace.
//! Each record stores the hash of the record before it (`prev`), and its own `hash` is
//! SHA-256 over its canonical JSON without the `hash` field. [`verify_file`] recomputes
//! the chain and reports the first bad sequence number.
//!
//! Limits: the chain detects edits, reordering and gaps. It cannot detect removing whole
//! records from the end of the file, or a rewritten file. Signed checkpoints and external
//! anchors (M9) close that gap.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tpt_shell_manifest::AuditSettings;

/// `prev` of the first record.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Value written in place of a redacted parameter.
pub const REDACTED: &str = "[redacted]";

/// Result of one command, as recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// The stable error code from the command, for example `IPC_DENIED`.
    Err(String),
}

/// One audited action, before it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub wall_unix_ms: u64,
    pub mono_ns: u64,
    pub actor: String,
    pub command: String,
    pub capability: Option<String>,
    /// Full parameters. Only their hash is always stored; see [`Policy`].
    pub params: Value,
    pub outcome: Outcome,
    pub duration_us: u64,
}

/// What the writer keeps from the parameters. The hash always covers the full parameters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// Keep a copy of the parameters, with `redact_keys` replaced. Off means only the hash.
    pub store_params: bool,
    /// Object keys whose values are replaced with [`REDACTED`], at any depth.
    pub redact_keys: Vec<String>,
}

/// The `[audit]` section of the manifest decides what the writer keeps, so its settings map
/// one-to-one onto a [`Policy`]. Both default to keeping only the hash.
impl From<&AuditSettings> for Policy {
    fn from(settings: &AuditSettings) -> Self {
        Policy {
            store_params: settings.store_params,
            redact_keys: settings.redact.clone(),
        }
    }
}

#[derive(Debug)]
pub enum AuditError {
    Io(String),
    /// The file is not a valid chain. `seq` is the first bad sequence number.
    Chain {
        seq: u64,
        reason: String,
    },
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::Io(msg) => write!(f, "audit file error: {msg}"),
            AuditError::Chain { seq, reason } => {
                write!(f, "audit chain broken at seq {seq}: {reason}")
            }
        }
    }
}

impl std::error::Error for AuditError {}

impl From<io::Error> for AuditError {
    fn from(e: io::Error) -> Self {
        AuditError::Io(e.to_string())
    }
}

/// Command name of checkpoint records. Their params hold `upto`, `hash` and `signature`.
pub const CHECKPOINT_COMMAND: &str = "tpt.checkpoint";

/// Signs a checkpoint every `every` records. The key signs the hash of the record it names.
pub struct Signing {
    pub key: SigningKey,
    pub every: u64,
}

/// Writer for one audit file. Opening an existing file verifies it first.
pub struct AuditLog {
    path: PathBuf,
    file: File,
    policy: Policy,
    signing: Option<Signing>,
    next_seq: u64,
    /// Entries written, not counting checkpoints. Drives the checkpoint interval.
    entries: u64,
    prev: String,
}

impl AuditLog {
    /// Opens (or creates) the file at `path`. An existing file must verify, or open fails.
    pub fn open(path: impl AsRef<Path>, policy: Policy) -> Result<Self, AuditError> {
        Self::open_inner(path.as_ref(), policy, None)
    }

    /// Like [`AuditLog::open`], but writes a signed checkpoint every `signing.every` records.
    /// An existing file is verified with the key's public half, signatures included.
    pub fn open_signed(
        path: impl AsRef<Path>,
        policy: Policy,
        signing: Signing,
    ) -> Result<Self, AuditError> {
        Self::open_inner(path.as_ref(), policy, Some(signing))
    }

    fn open_inner(
        path: &Path,
        policy: Policy,
        signing: Option<Signing>,
    ) -> Result<Self, AuditError> {
        let path = path.to_path_buf();
        let verifying = signing.as_ref().map(|s| s.key.verifying_key());
        let (next_seq, entries, prev) = if path.exists() {
            let summary = verify_inner(&path, verifying.as_ref())?;
            (
                summary.records,
                summary.records - summary.checkpoints,
                summary.last_hash,
            )
        } else {
            (0, 0, GENESIS.to_string())
        };
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(AuditLog {
            path,
            file,
            policy,
            signing,
            next_seq,
            entries,
            prev,
        })
    }

    /// Writes one record line and returns its hash.
    fn write_record(&mut self, record: Map<String, Value>) -> Result<String, AuditError> {
        let hash = hash_record(&record);
        let mut line = record;
        line.insert("hash".into(), Value::String(hash.clone()));
        let text = canonical(&Value::Object(line));
        self.file.write_all(text.as_bytes())?;
        self.file.write_all(b"\n")?;
        self.file.sync_data()?;
        Ok(hash)
    }

    /// Appends one record and returns its sequence number. The line is flushed to disk
    /// before this returns.
    pub fn append(&mut self, entry: &Entry) -> Result<u64, AuditError> {
        let seq = self.next_seq;
        let record = build_record(seq, &self.prev, entry, &self.policy);
        self.prev = self.write_record(record)?;
        self.next_seq += 1;
        self.entries += 1;
        // The interval counts entries, not checkpoints, so a checkpoint never triggers another.
        if let Some(every) = self.signing.as_ref().map(|s| s.every).filter(|e| *e > 0) {
            if self.entries.is_multiple_of(every) {
                self.append_checkpoint(seq, entry)?;
            }
        }
        Ok(seq)
    }

    /// Writes a signed checkpoint naming record `upto`, whose hash is the chain head.
    fn append_checkpoint(&mut self, upto: u64, around: &Entry) -> Result<(), AuditError> {
        let hash = self.prev.clone();
        let message = checkpoint_message(upto, &hash);
        let signature = match &self.signing {
            Some(s) => hex_encode(&s.key.sign(&message).to_bytes()),
            None => return Ok(()),
        };
        let checkpoint = Entry {
            wall_unix_ms: around.wall_unix_ms,
            mono_ns: around.mono_ns,
            actor: "tpt-shell".into(),
            command: CHECKPOINT_COMMAND.into(),
            capability: None,
            params: json!({ "upto": upto, "hash": hash, "signature": signature }),
            outcome: Outcome::Ok,
            duration_us: 0,
        };
        // Checkpoint params must always be stored, or the signature could not be checked.
        let keep = Policy {
            store_params: true,
            redact_keys: Vec::new(),
        };
        let seq = self.next_seq;
        let record = build_record(seq, &self.prev, &checkpoint, &keep);
        self.prev = self.write_record(record)?;
        self.next_seq += 1;
        Ok(())
    }

    /// Number of records written so far, including those found when the file was opened.
    pub fn len(&self) -> u64 {
        self.next_seq
    }

    pub fn is_empty(&self) -> bool {
        self.next_seq == 0
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a successful verification found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub records: u64,
    /// How many of the records are checkpoints.
    pub checkpoints: u64,
    pub last_hash: String,
}

/// Recomputes the whole chain in `path`. Checkpoints must name the record they claim to, but
/// their signatures are not checked, because no key is given. Use [`verify_file_with_key`].
pub fn verify_file(path: impl AsRef<Path>) -> Result<Summary, AuditError> {
    verify_inner(path.as_ref(), None)
}

/// Recomputes the chain and checks every checkpoint signature against `key`. A checkpoint
/// without a valid signature is reported at its own sequence number.
pub fn verify_file_with_key(
    path: impl AsRef<Path>,
    key: &VerifyingKey,
) -> Result<Summary, AuditError> {
    verify_inner(path.as_ref(), Some(key))
}

fn verify_inner(path: &Path, key: Option<&VerifyingKey>) -> Result<Summary, AuditError> {
    let reader = BufReader::new(File::open(path)?);
    let mut prev = GENESIS.to_string();
    let mut hashes: Vec<String> = Vec::new();
    let mut checkpoints: u64 = 0;
    let mut seq: u64 = 0;
    for line in reader.split(b'\n') {
        let line = line?;
        let bad = |reason: &str| AuditError::Chain {
            seq,
            reason: reason.to_string(),
        };
        let value: Value =
            serde_json::from_slice(&line).map_err(|_| bad("record is not valid JSON"))?;
        let Value::Object(mut record) = value else {
            return Err(bad("record is not an object"));
        };
        let stored_hash = match record.remove("hash") {
            Some(Value::String(h)) => h,
            _ => return Err(bad("record has no hash")),
        };
        if record.get("seq") != Some(&json!(seq)) {
            return Err(bad("sequence number does not match its position"));
        }
        if record.get("prev") != Some(&Value::String(prev.clone())) {
            return Err(bad("previous hash does not match"));
        }
        if hash_record(&record) != stored_hash {
            return Err(bad("hash does not match record contents"));
        }
        if record.get("command") == Some(&json!(CHECKPOINT_COMMAND)) {
            check_checkpoint(&record, &hashes, key).map_err(|reason| bad(&reason))?;
            checkpoints += 1;
        }
        hashes.push(stored_hash.clone());
        prev = stored_hash;
        seq += 1;
    }
    Ok(Summary {
        records: seq,
        checkpoints,
        last_hash: prev,
    })
}

/// Checks one checkpoint record: it names an earlier record by its hash, and when a key is
/// given, its signature is valid.
fn check_checkpoint(
    record: &Map<String, Value>,
    hashes: &[String],
    key: Option<&VerifyingKey>,
) -> Result<(), String> {
    let params = record
        .get("params")
        .and_then(Value::as_object)
        .ok_or("checkpoint has no params")?;
    let upto = params
        .get("upto")
        .and_then(Value::as_u64)
        .ok_or("checkpoint has no `upto`")?;
    let hash = params
        .get("hash")
        .and_then(Value::as_str)
        .ok_or("checkpoint has no `hash`")?;
    let named = usize::try_from(upto)
        .ok()
        .and_then(|i| hashes.get(i))
        .ok_or("checkpoint names a record that does not exist")?;
    if named != hash {
        return Err("checkpoint names a different record than its hash".into());
    }
    if let Some(key) = key {
        let signature = params
            .get("signature")
            .and_then(Value::as_str)
            .and_then(hex_decode)
            .ok_or("checkpoint signature is missing or malformed")?;
        let bytes: [u8; 64] = signature
            .try_into()
            .map_err(|_| "checkpoint signature has the wrong length".to_string())?;
        key.verify(
            &checkpoint_message(upto, hash),
            &Signature::from_bytes(&bytes),
        )
        .map_err(|_| "checkpoint signature is not valid for this key".to_string())?;
    }
    Ok(())
}

/// Reads a verifying key from its 64 hex digits, as written by a key file.
pub fn verifying_key_from_hex(text: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = hex_decode(text.trim())?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Column names of [`export_csv`], in order.
pub const CSV_HEADER: &str =
    "seq,wall_unix_ms,mono_ns,actor,command,capability,outcome,error_code,duration_us,params_sha256";

/// Verifies `path`, then renders one CSV row per record under [`CSV_HEADER`]. Parameters are
/// left out: the CSV is for review, and the file itself (JSON Lines) is the full record. A file
/// that does not verify exports nothing.
pub fn export_csv(path: impl AsRef<Path>) -> Result<String, AuditError> {
    let path = path.as_ref();
    verify_file(path)?;
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for line in BufReader::new(File::open(path)?).lines() {
        let record: Map<String, Value> = serde_json::from_str(&line?)
            .map_err(|_| AuditError::Io("verified record did not parse".into()))?;
        let cells: Vec<String> = CSV_HEADER
            .split(',')
            .map(|column| csv_cell(record.get(column)))
            .collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    Ok(out)
}

/// One CSV cell, quoted when it needs to be. Text that a spreadsheet would read as a formula
/// gets a leading apostrophe, because actor and command names come from the page.
fn csv_cell(value: Option<&Value>) -> String {
    let text = match value {
        None | Some(Value::Null) => return String::new(),
        Some(Value::String(s)) => {
            if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
                format!("'{s}")
            } else {
                s.clone()
            }
        }
        Some(other) => other.to_string(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// The bytes a checkpoint signature covers. The prefix keeps it apart from other signatures.
pub fn checkpoint_message(upto: u64, hash: &str) -> Vec<u8> {
    format!("tpt-shell-audit-checkpoint\n{upto}\n{hash}").into_bytes()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

fn build_record(seq: u64, prev: &str, entry: &Entry, policy: &Policy) -> Map<String, Value> {
    let params_hash = sha256_hex(canonical(&entry.params).as_bytes());
    let params = if policy.store_params {
        redact(&entry.params, &policy.redact_keys)
    } else {
        Value::Null
    };
    let (outcome, error_code) = match &entry.outcome {
        Outcome::Ok => ("ok", Value::Null),
        Outcome::Err(code) => ("err", Value::String(code.clone())),
    };
    let mut record = Map::new();
    record.insert("seq".into(), json!(seq));
    record.insert("wall_unix_ms".into(), json!(entry.wall_unix_ms));
    record.insert("mono_ns".into(), json!(entry.mono_ns));
    record.insert("actor".into(), json!(entry.actor));
    record.insert("command".into(), json!(entry.command));
    record.insert(
        "capability".into(),
        entry.capability.as_ref().map_or(Value::Null, |c| json!(c)),
    );
    record.insert("params_sha256".into(), json!(params_hash));
    record.insert("params".into(), params);
    record.insert("outcome".into(), json!(outcome));
    record.insert("error_code".into(), error_code);
    record.insert("duration_us".into(), json!(entry.duration_us));
    record.insert("prev".into(), json!(prev));
    record
}

fn hash_record(record: &Map<String, Value>) -> String {
    sha256_hex(canonical(&Value::Object(record.clone())).as_bytes())
}

fn redact(value: &Value, keys: &[String]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let v = if keys.contains(k) {
                        json!(REDACTED)
                    } else {
                        redact(v, keys)
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| redact(v, keys)).collect()),
        other => other.clone(),
    }
}

/// Canonical JSON: object keys sorted by their UTF-8 bytes, no whitespace, otherwise the
/// standard serializer's output. Two equal values always produce the same bytes.
pub fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
    }
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tpt-audit-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn entry(n: u64) -> Entry {
        Entry {
            wall_unix_ms: 1_700_000_000_000 + n,
            mono_ns: n * 1000,
            actor: "page".into(),
            command: format!("cmd_{n}"),
            capability: Some("fs.read".into()),
            params: json!({ "path": format!("C:/data/{n}.txt"), "password": "hunter2" }),
            outcome: if n.is_multiple_of(3) {
                Outcome::Err("IPC_DENIED".into())
            } else {
                Outcome::Ok
            },
            duration_us: 40 + n,
        }
    }

    fn write_n(path: &Path, n: u64) {
        let mut log = AuditLog::open(path, Policy::default()).unwrap();
        for i in 0..n {
            log.append(&entry(i)).unwrap();
        }
    }

    #[test]
    fn appended_records_verify() {
        let path = temp_path("chain.jsonl");
        write_n(&path, 5);
        let summary = verify_file(&path).unwrap();
        assert_eq!(summary.records, 5);
    }

    #[test]
    fn reopening_continues_the_chain() {
        let path = temp_path("reopen.jsonl");
        write_n(&path, 2);
        let mut log = AuditLog::open(&path, Policy::default()).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log.append(&entry(2)).unwrap(), 2);
        assert_eq!(verify_file(&path).unwrap().records, 3);
    }

    #[test]
    fn open_refuses_a_broken_file() {
        let path = temp_path("broken-open.jsonl");
        write_n(&path, 3);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("cmd_1", "cmd_x")).unwrap();
        assert!(matches!(
            AuditLog::open(&path, Policy::default()),
            Err(AuditError::Chain { seq: 1, .. })
        ));
    }

    #[test]
    fn a_single_changed_byte_is_detected_at_its_record() {
        let path = temp_path("flip.jsonl");
        write_n(&path, 4);
        let text = std::fs::read_to_string(&path).unwrap();
        let tampered = text.replacen("\"duration_us\":41", "\"duration_us\":42", 1);
        assert_ne!(tampered, text);
        std::fs::write(&path, tampered).unwrap();
        match verify_file(&path) {
            Err(AuditError::Chain { seq, .. }) => assert_eq!(seq, 1),
            other => panic!("expected chain error at seq 1, got {other:?}"),
        }
    }

    #[test]
    fn a_dropped_middle_record_is_detected() {
        let path = temp_path("drop.jsonl");
        write_n(&path, 4);
        let text = std::fs::read_to_string(&path).unwrap();
        let kept: Vec<&str> = text
            .lines()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| l)
            .collect();
        std::fs::write(&path, kept.join("\n") + "\n").unwrap();
        assert!(matches!(
            verify_file(&path),
            Err(AuditError::Chain { seq: 1, .. })
        ));
    }

    #[test]
    fn swapped_records_are_detected() {
        let path = temp_path("swap.jsonl");
        write_n(&path, 3);
        let text = std::fs::read_to_string(&path).unwrap();
        let mut lines: Vec<&str> = text.lines().collect();
        lines.swap(0, 1);
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        assert!(matches!(
            verify_file(&path),
            Err(AuditError::Chain { seq: 0, .. })
        ));
    }

    #[test]
    fn a_partial_last_line_is_detected() {
        let path = temp_path("partial.jsonl");
        write_n(&path, 2);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.truncate(text.len() - 20);
        std::fs::write(&path, text).unwrap();
        assert!(matches!(
            verify_file(&path),
            Err(AuditError::Chain { seq: 1, .. })
        ));
    }

    #[test]
    fn every_single_byte_change_is_detected() {
        // Exhaustive over a small chain: each byte is replaced by a different ASCII letter.
        let path = temp_path("exhaustive.jsonl");
        write_n(&path, 3);
        let original = std::fs::read(&path).unwrap();
        for i in 0..original.len() {
            let mut changed = original.clone();
            changed[i] = if original[i] == b'x' { b'y' } else { b'x' };
            std::fs::write(&path, &changed).unwrap();
            assert!(
                verify_file(&path).is_err(),
                "change at byte {i} was not detected"
            );
        }
    }

    #[test]
    fn random_chains_verify_and_random_edits_are_caught() {
        // Small xorshift generator: deterministic, no dependency.
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..20 {
            let path = temp_path(&format!("random-{round}.jsonl"));
            let n = 1 + next() % 12;
            write_n(&path, n);
            assert_eq!(verify_file(&path).unwrap().records, n);

            let original = std::fs::read(&path).unwrap();
            let at = (next() as usize) % original.len();
            let mut changed = original.clone();
            changed[at] = if original[at] == b'q' { b'r' } else { b'q' };
            std::fs::write(&path, &changed).unwrap();
            assert!(
                verify_file(&path).is_err(),
                "round {round}: edit at {at} not caught"
            );
        }
    }

    #[test]
    fn redaction_removes_values_but_the_hash_covers_the_original() {
        let path = temp_path("redact.jsonl");
        let policy = Policy {
            store_params: true,
            redact_keys: vec!["password".into()],
        };
        let mut log = AuditLog::open(&path, policy).unwrap();
        log.append(&entry(1)).unwrap();
        drop(log);

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("hunter2"));
        assert!(text.contains(REDACTED));
        assert!(verify_file(&path).is_ok());

        let record: Value = serde_json::from_str(text.trim()).unwrap();
        let expected = sha256_hex(canonical(&entry(1).params).as_bytes());
        assert_eq!(record["params_sha256"], Value::String(expected));
    }

    #[test]
    fn manifest_audit_settings_map_onto_the_policy() {
        assert_eq!(Policy::from(&AuditSettings::default()), Policy::default());
        let settings = AuditSettings {
            store_params: true,
            redact: vec!["password".into()],
        };
        assert_eq!(
            Policy::from(&settings),
            Policy {
                store_params: true,
                redact_keys: vec!["password".into()],
            }
        );
    }

    #[test]
    fn without_store_params_only_the_hash_is_kept() {
        let path = temp_path("no-params.jsonl");
        let mut log = AuditLog::open(&path, Policy::default()).unwrap();
        log.append(&entry(1)).unwrap();
        drop(log);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("C:/data"));
        assert!(text.contains("\"params\":null"));
    }

    #[test]
    fn canonical_json_sorts_keys_and_drops_whitespace() {
        let v = json!({ "b": [1, { "d": true, "c": null }], "a": "x" });
        assert_eq!(canonical(&v), r#"{"a":"x","b":[1,{"c":null,"d":true}]}"#);
    }

    #[test]
    fn golden_record_bytes_and_hash() {
        // A fixed record must always serialize to the same bytes and hash to the same digest.
        let record = build_record(
            0,
            GENESIS,
            &Entry {
                wall_unix_ms: 1,
                mono_ns: 2,
                actor: "page".into(),
                command: "ping".into(),
                capability: None,
                params: json!({}),
                outcome: Outcome::Ok,
                duration_us: 3,
            },
            &Policy::default(),
        );
        let text = canonical(&Value::Object(record.clone()));
        assert_eq!(
            text,
            format!(
                r#"{{"actor":"page","capability":null,"command":"ping","duration_us":3,"error_code":null,"mono_ns":2,"outcome":"ok","params":null,"params_sha256":"{}","prev":"{GENESIS}","seq":0,"wall_unix_ms":1}}"#,
                sha256_hex(b"{}")
            )
        );
        assert_eq!(hash_record(&record), GOLDEN_HASH);
    }

    /// SHA-256 of the golden record's canonical JSON, checked independently with sha256sum.
    const GOLDEN_HASH: &str = "1dd21dcb478af0647195cc398b1368a4c857cf6e0cbc378dddc890b2a319db1b";
}

#[cfg(test)]
mod signed_tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("tpt-audit-signed-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn entry(n: u64) -> Entry {
        Entry {
            wall_unix_ms: 1_700_000_000_000 + n,
            mono_ns: n * 1000,
            actor: "page".into(),
            command: format!("cmd_{n}"),
            capability: None,
            params: json!({ "n": n }),
            outcome: Outcome::Ok,
            duration_us: 5,
        }
    }

    fn write_signed(path: &Path, records: u64, every: u64) {
        let signing = Signing { key: key(1), every };
        let mut log = AuditLog::open_signed(path, Policy::default(), signing).unwrap();
        for i in 0..records {
            log.append(&entry(i)).unwrap();
        }
    }

    fn lines(path: &Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn save(path: &Path, lines: &[String]) {
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn checkpoints_are_written_every_n_records_and_verify_with_the_key() {
        let path = temp_path("every-3.jsonl");
        write_signed(&path, 7, 3);
        let text = std::fs::read_to_string(&path).unwrap();
        let checkpoints = text.matches(CHECKPOINT_COMMAND).count();
        // 7 entries plus checkpoints after entries 2 and 5: 9 records, 2 of them checkpoints.
        assert_eq!(checkpoints, 2);
        assert_eq!(lines(&path).len(), 9);
        let summary = verify_file_with_key(&path, &key(1).verifying_key()).unwrap();
        assert_eq!(summary.records, 9);
    }

    #[test]
    fn reopening_a_signed_log_continues_it() {
        let path = temp_path("reopen-signed.jsonl");
        write_signed(&path, 3, 2);
        write_signed(&path, 3, 2);
        assert!(verify_file_with_key(&path, &key(1).verifying_key()).is_ok());
    }

    #[test]
    fn a_wrong_key_is_refused_at_open_and_at_verify() {
        let path = temp_path("wrong-key.jsonl");
        write_signed(&path, 4, 2);
        assert!(verify_file_with_key(&path, &key(2).verifying_key()).is_err());
        let wrong = Signing {
            key: key(2),
            every: 2,
        };
        assert!(AuditLog::open_signed(&path, Policy::default(), wrong).is_err());
    }

    #[test]
    fn a_rechained_edit_is_caught_by_the_checkpoint_hash() {
        // An attacker without the key edits a record and recomputes every hash after it.
        let path = temp_path("rechain.jsonl");
        write_signed(&path, 6, 3);
        let mut rewritten = Vec::new();
        let mut prev = GENESIS.to_string();
        for (i, line) in lines(&path).iter().enumerate() {
            let mut record: Map<String, Value> = serde_json::from_str(line).unwrap();
            record.remove("hash");
            if i == 1 {
                record.insert("duration_us".into(), json!(999));
            }
            record.insert("prev".into(), json!(prev));
            let hash = hash_record(&record);
            record.insert("hash".into(), json!(hash));
            prev = hash;
            rewritten.push(canonical(&Value::Object(record)));
        }
        save(&path, &rewritten);
        // Unsigned verification sees a consistent chain, but the checkpoint names the old hash.
        assert!(matches!(verify_file(&path), Err(AuditError::Chain { .. })));
        assert!(verify_file_with_key(&path, &key(1).verifying_key()).is_err());
    }

    #[test]
    fn a_forged_checkpoint_fails_the_signature_check() {
        // With every = 2 and four entries, checkpoints sit at sequences 2 and 5. The attacker
        // zeroes the signature of the first one and recomputes every hash after it. The later
        // checkpoint still names the old hash of record 4, so an unsigned check fails at 5. A
        // signed check fails at 2, where the signature is wrong.
        let path = temp_path("forged.jsonl");
        write_signed(&path, 4, 2);
        let mut rewritten: Vec<String> = Vec::new();
        let mut prev = GENESIS.to_string();
        for (i, line) in lines(&path).iter().enumerate() {
            let mut record: Map<String, Value> = serde_json::from_str(line).unwrap();
            record.remove("hash");
            if i == 2 {
                let mut params = record["params"].as_object().unwrap().clone();
                params.insert("signature".into(), json!("00".repeat(64)));
                record.insert("params".into(), Value::Object(params));
                let canonical_params = canonical(&record["params"]);
                record.insert(
                    "params_sha256".into(),
                    json!(sha256_hex(canonical_params.as_bytes())),
                );
            }
            record.insert("prev".into(), json!(prev));
            let hash = hash_record(&record);
            record.insert("hash".into(), json!(hash));
            prev = hash;
            rewritten.push(canonical(&Value::Object(record)));
        }
        save(&path, &rewritten);
        assert!(
            matches!(verify_file(&path), Err(AuditError::Chain { seq: 5, .. })),
            "without the key, the later checkpoint still catches the rewrite"
        );
        match verify_file_with_key(&path, &key(1).verifying_key()) {
            Err(AuditError::Chain { seq, reason }) => {
                assert_eq!(seq, 2);
                assert!(reason.contains("signature"), "{reason}");
            }
            other => panic!("expected a signature failure at seq 2, got {other:?}"),
        }
    }

    #[test]
    fn dropping_the_tail_is_not_detected_by_the_chain_alone() {
        // Documented limit: removing records from the end leaves a valid chain. Only an
        // external anchor (later milestone) can detect it.
        let path = temp_path("tail.jsonl");
        write_signed(&path, 5, 2);
        let mut kept = lines(&path);
        kept.pop();
        save(&path, &kept);
        assert!(verify_file_with_key(&path, &key(1).verifying_key()).is_ok());
    }

    #[test]
    fn hex_decoding_refuses_bad_input() {
        assert_eq!(hex_decode("0a0b"), Some(vec![10, 11]));
        assert_eq!(hex_decode("0"), None);
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode("é0"), None);
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("tpt-audit-export-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn entry(command: &str) -> Entry {
        Entry {
            wall_unix_ms: 1_700_000_000_123,
            mono_ns: 42,
            actor: "page".into(),
            command: command.into(),
            capability: Some("fs.read".into()),
            params: json!({ "path": "a,b" }),
            outcome: Outcome::Err("IPC_DENIED".into()),
            duration_us: 7,
        }
    }

    fn write(path: &Path, entries: &[Entry]) {
        let mut log = AuditLog::open(path, Policy::default()).unwrap();
        for e in entries {
            log.append(e).unwrap();
        }
    }

    #[test]
    fn csv_has_the_header_and_one_row_per_record() {
        let path = temp_path("log.jsonl");
        write(&path, &[entry("open"), entry("save")]);
        let csv = export_csv(&path).unwrap();
        let rows: Vec<&str> = csv.lines().collect();
        assert_eq!(rows[0], CSV_HEADER);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[1],
            format!(
                "0,1700000000123,42,page,open,fs.read,err,IPC_DENIED,7,{}",
                sha256_hex(canonical(&json!({ "path": "a,b" })).as_bytes())
            )
        );
        assert!(rows[2].starts_with("1,1700000000123,42,page,save,"));
    }

    #[test]
    fn csv_quotes_cells_with_commas_and_neutralises_formulas() {
        assert_eq!(csv_cell(Some(&json!("a,b"))), "\"a,b\"");
        assert_eq!(csv_cell(Some(&json!("say \"hi\""))), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell(Some(&json!("=HYPERLINK(x)"))), "'=HYPERLINK(x)");
        assert_eq!(csv_cell(Some(&json!("-1+1"))), "'-1+1");
        assert_eq!(csv_cell(Some(&json!(-5))), "-5");
        assert_eq!(csv_cell(Some(&Value::Null)), "");
        assert_eq!(csv_cell(None), "");
    }

    #[test]
    fn a_broken_chain_exports_nothing() {
        let path = temp_path("broken.jsonl");
        write(&path, &[entry("open"), entry("save")]);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replacen("\"open\"", "\"oped\"", 1)).unwrap();
        assert!(matches!(
            export_csv(&path),
            Err(AuditError::Chain { seq: 0, .. })
        ));
    }

    #[test]
    fn a_hex_key_file_round_trips_into_a_verifying_key() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let hex = hex_encode(key.verifying_key().as_bytes());
        assert_eq!(
            verifying_key_from_hex(&format!("{hex}\n")),
            Some(key.verifying_key())
        );
        assert_eq!(verifying_key_from_hex("abcd"), None);
        assert_eq!(verifying_key_from_hex(&"zz".repeat(32)), None);
    }
}
