// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The test driver's control channel (spec §10.6, R7). Compiled only with the `test-driver`
//! feature.
//!
//! The channel is a directory, not a socket or a named pipe. A run creates one directory,
//! named by a 128-bit per-run token, inside a base directory the caller chooses (it must be
//! private to the user). The driver writes `req-<seq>.json` files; the app writes
//! `res-<seq>.json` files in reply. Screenshots go to `artifacts/`. Each file is written under
//! a temporary name and renamed into place, so a reader never sees a half-written message.
//!
//! The app side checks every message before it acts on it:
//! - the token must match the run's token (compared in constant time);
//! - the protocol version must be [`PROTOCOL`];
//! - the file must be at most [`MAX_MESSAGE_BYTES`];
//! - the sequence number must be the one the app expects next;
//! - the command must be one of the known commands, with arguments inside their limits;
//! - file names come only from integers or a fixed pattern, so no path can leave the run
//!   directory.
//!
//! Anything else is refused with a `TS0301` error, and nothing is executed.
//!
//! Support: Full on Windows, where it is built and tested here. The channel is plain files, so
//! it runs on every platform, but the app-side runner that answers requests is routed to the
//! facade and the platform crates.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tpt_shell_core::{Rng, SystemRng};

/// The only protocol version this build speaks.
pub const PROTOCOL: u64 = 1;
/// The largest request or response file accepted, in bytes.
pub const MAX_MESSAGE_BYTES: u64 = 64 * 1024;
/// The largest screenshot file the driver reads back, in bytes.
pub const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;
/// The longest accessible name or text argument, in characters.
pub const MAX_TEXT_CHARS: usize = 1024;
/// The longest script `eval` accepts, in bytes.
pub const MAX_SCRIPT_BYTES: usize = 16 * 1024;

/// A refused message or a failed channel operation. `code` is a stable `TS####` code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelError {
    pub code: &'static str,
    pub message: String,
}

impl ChannelError {
    fn refused(message: impl Into<String>) -> Self {
        ChannelError {
            code: "TS0301",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ChannelError {}

/// A per-run token: 128 random bits as 32 lowercase hex characters.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// A fresh token from the process's random source. Support: Full.
    pub fn generate() -> Self {
        let mut rng = SystemRng::new();
        Token(format!("{:016x}{:016x}", rng.next_u64(), rng.next_u64()))
    }

    /// Reads a token from text. It must be exactly 32 lowercase hex characters. Support: Full.
    pub fn parse(text: &str) -> Result<Self, ChannelError> {
        let ok = text.len() == 32
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if ok {
            Ok(Token(text.to_string()))
        } else {
            Err(ChannelError::refused(
                "the token is not 32 lowercase hex characters",
            ))
        }
    }

    /// The token text. It is secret for the run, so do not print it in logs.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Compares with a token from a message, in time that does not depend on where they
    /// differ. Support: Full.
    pub fn matches(&self, other: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), other.as_bytes());
        if a.len() != b.len() {
            return false;
        }
        a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// A command the driver can send. Each one is checked by [`Command::from_json`].
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Finds the first element with this role, and with this name when one is given.
    Find {
        role: String,
        name: Option<String>,
    },
    Click {
        handle: u64,
    },
    TypeText {
        handle: u64,
        text: String,
    },
    /// Presses a key by name, such as `Enter`, `Tab` or `a`.
    Press {
        key: String,
    },
    /// Captures the window to `artifacts/<name>.png`.
    Screenshot {
        name: String,
    },
    DomSnapshot,
    A11yTree,
    /// Runs a script in the page and returns its JSON result. Test builds only.
    Eval {
        script: String,
    },
}

impl Command {
    /// The command as a JSON object, with its `cmd` field.
    pub fn to_json(&self) -> Value {
        match self {
            Command::Find { role, name } => json!({ "cmd": "find", "role": role, "name": name }),
            Command::Click { handle } => json!({ "cmd": "click", "handle": handle }),
            Command::TypeText { handle, text } => {
                json!({ "cmd": "type_text", "handle": handle, "text": text })
            }
            Command::Press { key } => json!({ "cmd": "press", "key": key }),
            Command::Screenshot { name } => json!({ "cmd": "screenshot", "name": name }),
            Command::DomSnapshot => json!({ "cmd": "dom_snapshot" }),
            Command::A11yTree => json!({ "cmd": "a11y_tree" }),
            Command::Eval { script } => json!({ "cmd": "eval", "script": script }),
        }
    }

    /// Reads and checks a command. Unknown fields and out-of-range values are refused.
    pub fn from_json(value: &Value) -> Result<Self, ChannelError> {
        let obj = value
            .as_object()
            .ok_or_else(|| ChannelError::refused("command is not an object"))?;
        let cmd = str_field(obj, "cmd")?;
        let allowed: &[&str] = match cmd {
            "find" => &["cmd", "role", "name"],
            "click" => &["cmd", "handle"],
            "type_text" => &["cmd", "handle", "text"],
            "press" => &["cmd", "key"],
            "screenshot" => &["cmd", "name"],
            "dom_snapshot" | "a11y_tree" => &["cmd"],
            "eval" => &["cmd", "script"],
            other => return Err(ChannelError::refused(format!("unknown command `{other}`"))),
        };
        if let Some(extra) = obj.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(ChannelError::refused(format!(
                "`{cmd}` has an unknown field `{extra}`"
            )));
        }
        Ok(match cmd {
            "find" => {
                let role = str_field(obj, "role")?;
                if role.is_empty()
                    || role.len() > 32
                    || !role.bytes().all(|b| b.is_ascii_lowercase())
                {
                    return Err(ChannelError::refused("role must be 1-32 lowercase letters"));
                }
                let name = match obj.get("name") {
                    None | Some(Value::Null) => None,
                    Some(_) => Some(text_field(obj, "name", MAX_TEXT_CHARS)?),
                };
                Command::Find {
                    role: role.to_string(),
                    name,
                }
            }
            "click" => Command::Click {
                handle: handle_field(obj)?,
            },
            "type_text" => Command::TypeText {
                handle: handle_field(obj)?,
                text: text_field(obj, "text", MAX_TEXT_CHARS)?,
            },
            "press" => {
                let key = str_field(obj, "key")?;
                let ok = !key.is_empty()
                    && key.len() <= 32
                    && key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'_' | b'-'));
                if !ok {
                    return Err(ChannelError::refused("key name is not valid"));
                }
                Command::Press {
                    key: key.to_string(),
                }
            }
            "screenshot" => {
                let name = str_field(obj, "name")?;
                if !is_artifact_stem(name) {
                    return Err(ChannelError::refused("screenshot name is not valid"));
                }
                Command::Screenshot {
                    name: name.to_string(),
                }
            }
            "dom_snapshot" => Command::DomSnapshot,
            "a11y_tree" => Command::A11yTree,
            _ => {
                let script = str_field(obj, "script")?;
                if script.len() > MAX_SCRIPT_BYTES {
                    return Err(ChannelError::refused("script is too long"));
                }
                Command::Eval {
                    script: script.to_string(),
                }
            }
        })
    }
}

fn str_field<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, ChannelError> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ChannelError::refused(format!("`{key}` must be a string")))
}

fn text_field(
    obj: &Map<String, Value>,
    key: &str,
    max_chars: usize,
) -> Result<String, ChannelError> {
    let text = str_field(obj, key)?;
    if text.chars().count() > max_chars {
        return Err(ChannelError::refused(format!(
            "`{key}` is longer than {max_chars} characters"
        )));
    }
    if text.contains('\0') {
        return Err(ChannelError::refused(format!(
            "`{key}` contains a NUL character"
        )));
    }
    Ok(text.to_string())
}

fn handle_field(obj: &Map<String, Value>) -> Result<u64, ChannelError> {
    obj.get("handle")
        .and_then(Value::as_u64)
        .ok_or_else(|| ChannelError::refused("`handle` must be a whole number"))
}

/// A name that is safe as a file stem: 1-100 ASCII letters, digits, `-` or `_`.
pub fn is_artifact_stem(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A request as the app receives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub seq: u64,
    pub command: Command,
}

/// Encodes a request. Support: Full.
pub fn encode_request(token: &Token, seq: u64, command: &Command) -> Vec<u8> {
    let mut obj = Map::new();
    obj.insert("v".into(), json!(PROTOCOL));
    obj.insert("token".into(), json!(token.as_str()));
    obj.insert("seq".into(), json!(seq));
    obj.insert("command".into(), command.to_json());
    Value::Object(obj).to_string().into_bytes()
}

/// Checks and decodes a request on the app side. `expected_seq` is the next sequence number
/// the app will accept, so a replayed or skipped message is refused. Support: Full.
pub fn parse_request(
    token: &Token,
    expected_seq: u64,
    bytes: &[u8],
) -> Result<Request, ChannelError> {
    let value = parse_envelope(bytes)?;
    let obj = value
        .as_object()
        .ok_or_else(|| ChannelError::refused("request is not an object"))?;
    check_common(obj, token)?;
    let seq = obj
        .get("seq")
        .and_then(Value::as_u64)
        .ok_or_else(|| ChannelError::refused("`seq` must be a whole number"))?;
    if seq != expected_seq {
        return Err(ChannelError::refused(format!(
            "sequence {seq} arrived where {expected_seq} was expected"
        )));
    }
    let command = Command::from_json(
        obj.get("command")
            .ok_or_else(|| ChannelError::refused("request has no `command`"))?,
    )?;
    Ok(Request { seq, command })
}

/// A response as the driver receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    Ok(Value),
    Err { code: String, message: String },
}

/// Encodes a response on the app side. Support: Full.
pub fn encode_response(token: &Token, seq: u64, result: &Result<Value, ChannelError>) -> Vec<u8> {
    let body = match result {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(e) => json!({ "ok": false, "code": e.code, "message": e.message }),
    };
    json!({ "v": PROTOCOL, "token": token.as_str(), "seq": seq, "result": body })
        .to_string()
        .into_bytes()
}

/// Checks and decodes a response on the driver side. Support: Full.
pub fn parse_response(token: &Token, seq: u64, bytes: &[u8]) -> Result<Response, ChannelError> {
    let value = parse_envelope(bytes)?;
    let obj = value
        .as_object()
        .ok_or_else(|| ChannelError::refused("response is not an object"))?;
    check_common(obj, token)?;
    if obj.get("seq").and_then(Value::as_u64) != Some(seq) {
        return Err(ChannelError::refused(
            "response has the wrong sequence number",
        ));
    }
    let result = obj
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| ChannelError::refused("response has no `result`"))?;
    match result.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(Response::Ok(
            result.get("data").cloned().unwrap_or(Value::Null),
        )),
        Some(false) => Ok(Response::Err {
            code: result
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("TS0301")
                .to_string(),
            message: result
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(MAX_TEXT_CHARS)
                .collect(),
        }),
        None => Err(ChannelError::refused("response has no boolean `ok`")),
    }
}

fn parse_envelope(bytes: &[u8]) -> Result<Value, ChannelError> {
    if bytes.len() as u64 > MAX_MESSAGE_BYTES {
        return Err(ChannelError::refused(
            "message is larger than the channel allows",
        ));
    }
    serde_json::from_slice(bytes).map_err(|_| ChannelError::refused("message is not JSON"))
}

fn check_common(obj: &Map<String, Value>, token: &Token) -> Result<(), ChannelError> {
    if obj.get("v").and_then(Value::as_u64) != Some(PROTOCOL) {
        return Err(ChannelError::refused("unsupported protocol version"));
    }
    let given = obj
        .get("token")
        .and_then(Value::as_str)
        .ok_or_else(|| ChannelError::refused("message has no token"))?;
    if !token.matches(given) {
        return Err(ChannelError::refused("token does not match this run"));
    }
    Ok(())
}

/// The file names a run uses. Only integers appear in them, so no path can be built from
/// message content.
pub fn request_file(seq: u64) -> String {
    format!("req-{seq:08}.json")
}

/// The response file name for a sequence number.
pub fn response_file(seq: u64) -> String {
    format!("res-{seq:08}.json")
}

/// The driver's end of a run directory. It writes requests and reads responses.
pub struct FileChannel {
    dir: PathBuf,
    token: Token,
    next_seq: u64,
}

impl FileChannel {
    /// Creates a new run directory named by a fresh token under `base`. It fails if the
    /// directory already exists, so a run never reuses another run's files. Support: Full.
    pub fn create(base: &Path) -> Result<Self, ChannelError> {
        let token = Token::generate();
        let dir = base.join(format!("run-{}", token.as_str()));
        fs::create_dir_all(base).map_err(io_error)?;
        fs::create_dir(&dir).map_err(io_error)?;
        fs::create_dir(dir.join("artifacts")).map_err(io_error)?;
        Ok(FileChannel {
            dir,
            token,
            next_seq: 0,
        })
    }

    /// The run's token. The launcher passes it to the app, for example in an environment
    /// variable, and never writes it to a log.
    pub fn token(&self) -> &Token {
        &self.token
    }

    /// The run directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Sends a command and waits for its response, up to `timeout`. Support: Full.
    pub fn call(&mut self, command: &Command, timeout: Duration) -> Result<Value, ChannelError> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let bytes = encode_request(&self.token, seq, command);
        write_atomic(&self.dir.join(request_file(seq)), &bytes)?;
        let path = self.dir.join(response_file(seq));
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(meta) = fs::metadata(&path) {
                if meta.len() > MAX_MESSAGE_BYTES {
                    return Err(ChannelError::refused(
                        "response is larger than the channel allows",
                    ));
                }
                let bytes = fs::read(&path).map_err(io_error)?;
                let _ = fs::remove_file(&path);
                return match parse_response(&self.token, seq, &bytes)? {
                    Response::Ok(data) => Ok(data),
                    Response::Err { code, message } => Err(ChannelError {
                        code: static_code(&code),
                        message,
                    }),
                };
            }
            if Instant::now() >= deadline {
                return Err(ChannelError {
                    code: "TS0302",
                    message: format!(
                        "no response to `{}` within {timeout:?}",
                        command_name(command)
                    ),
                });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Reads a file from `artifacts/`. The name must be a single file name ending in `.png`
    /// and the file must be at most [`MAX_ARTIFACT_BYTES`]. Support: Full.
    pub fn read_artifact(&self, name: &str) -> Result<Vec<u8>, ChannelError> {
        let stem = name.strip_suffix(".png").unwrap_or("");
        if !is_artifact_stem(stem) {
            return Err(ChannelError {
                code: "TS0305",
                message: "artifact name is not a plain file name".into(),
            });
        }
        let path = self.dir.join("artifacts").join(name);
        let meta = fs::metadata(&path).map_err(io_error)?;
        if meta.len() > MAX_ARTIFACT_BYTES {
            return Err(ChannelError {
                code: "TS0305",
                message: "artifact is larger than the channel allows".into(),
            });
        }
        fs::read(&path).map_err(io_error)
    }
}

/// The app's end of a run directory: it reads requests and writes responses.
pub struct AppChannel {
    dir: PathBuf,
    token: Token,
    next_seq: u64,
}

impl AppChannel {
    /// Opens the run directory the driver created, with the token it was given. Support: Full.
    pub fn open(dir: &Path, token: &str) -> Result<Self, ChannelError> {
        let token = Token::parse(token)?;
        let expected = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if expected != format!("run-{}", token.as_str()) {
            return Err(ChannelError::refused(
                "the run directory does not match the token",
            ));
        }
        Ok(AppChannel {
            dir: dir.to_path_buf(),
            token,
            next_seq: 0,
        })
    }

    /// The next request, if the driver has written one. A request that fails the checks is
    /// answered with an error and skipped, so the app never runs it. Support: Full.
    pub fn poll(&mut self) -> Option<Result<Request, ChannelError>> {
        let path = self.dir.join(request_file(self.next_seq));
        let meta = fs::metadata(&path).ok()?;
        if meta.len() > MAX_MESSAGE_BYTES {
            return Some(Err(ChannelError::refused(
                "request is larger than the channel allows",
            )));
        }
        let bytes = fs::read(&path).ok()?;
        let _ = fs::remove_file(&path);
        let seq = self.next_seq;
        self.next_seq += 1;
        Some(parse_request(&self.token, seq, &bytes))
    }

    /// Writes the answer to request `seq`. Support: Full.
    pub fn respond(
        &self,
        seq: u64,
        result: &Result<Value, ChannelError>,
    ) -> Result<(), ChannelError> {
        write_atomic(
            &self.dir.join(response_file(seq)),
            &encode_response(&self.token, seq, result),
        )
    }

    /// Writes a screenshot or other artifact under `artifacts/`. The name is checked as
    /// [`FileChannel::read_artifact`] checks it. Support: Full.
    pub fn write_artifact(&self, name: &str, bytes: &[u8]) -> Result<(), ChannelError> {
        if !is_artifact_stem(name) {
            return Err(ChannelError {
                code: "TS0305",
                message: "artifact name is not valid".into(),
            });
        }
        write_atomic(
            &self.dir.join("artifacts").join(format!("{name}.png")),
            bytes,
        )
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Find { .. } => "find",
        Command::Click { .. } => "click",
        Command::TypeText { .. } => "type_text",
        Command::Press { .. } => "press",
        Command::Screenshot { .. } => "screenshot",
        Command::DomSnapshot => "dom_snapshot",
        Command::A11yTree => "a11y_tree",
        Command::Eval { .. } => "eval",
    }
}

/// Maps a code from the app's response back to a static string. Unknown codes become the
/// generic refusal code.
fn static_code(code: &str) -> &'static str {
    const KNOWN: [&str; 5] = ["TS0301", "TS0302", "TS0303", "TS0304", "TS0305"];
    KNOWN
        .iter()
        .find(|k| **k == code)
        .copied()
        .unwrap_or("TS0301")
}

fn io_error(e: std::io::Error) -> ChannelError {
    ChannelError {
        code: "TS0301",
        message: format!("channel file error: {e}"),
    }
}

/// Writes to a temporary name in the same directory, then renames it into place, so a reader
/// never sees a partial file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ChannelError> {
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
    }
    fs::rename(&tmp, path).map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> Token {
        Token::parse("0123456789abcdef0123456789abcdef").unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let base =
            std::env::temp_dir().join(format!("tpt-shell-channel-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        base
    }

    #[test]
    fn tokens_are_32_lowercase_hex_and_differ_between_runs() {
        let a = Token::generate();
        let b = Token::generate();
        assert_eq!(a.as_str().len(), 32);
        assert!(a
            .as_str()
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(a, b);
    }

    #[test]
    fn malformed_tokens_are_refused() {
        for bad in [
            "",
            "abc",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcdeg",
            "../../../../../../../../../../etc/pass",
        ] {
            assert!(Token::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn token_comparison_is_exact() {
        let t = token();
        assert!(t.matches("0123456789abcdef0123456789abcdef"));
        assert!(!t.matches("0123456789abcdef0123456789abcdee"));
        assert!(!t.matches("0123456789abcdef"));
    }

    #[test]
    fn a_token_does_not_print_its_value() {
        assert_eq!(format!("{:?}", token()), "Token(..)");
    }

    #[test]
    fn a_request_round_trips() {
        let cmd = Command::Find {
            role: "button".into(),
            name: Some("Compress".into()),
        };
        let bytes = encode_request(&token(), 0, &cmd);
        let req = parse_request(&token(), 0, &bytes).unwrap();
        assert_eq!(
            req,
            Request {
                seq: 0,
                command: cmd
            }
        );
    }

    #[test]
    fn a_request_with_the_wrong_token_is_refused() {
        let other = Token::parse("ffffffffffffffffffffffffffffffff").unwrap();
        let bytes = encode_request(&other, 0, &Command::DomSnapshot);
        let err = parse_request(&token(), 0, &bytes).unwrap_err();
        assert_eq!(err.code, "TS0301");
    }

    #[test]
    fn a_replayed_or_skipped_sequence_number_is_refused() {
        let bytes = encode_request(&token(), 3, &Command::DomSnapshot);
        assert!(parse_request(&token(), 0, &bytes).is_err());
        assert!(parse_request(&token(), 3, &bytes).is_ok());
    }

    #[test]
    fn unknown_commands_and_extra_fields_are_refused() {
        let unknown = json!({ "v": 1, "token": token().as_str(), "seq": 0,
            "command": { "cmd": "shutdown" } });
        assert!(parse_request(&token(), 0, unknown.to_string().as_bytes()).is_err());
        let extra = json!({ "v": 1, "token": token().as_str(), "seq": 0,
            "command": { "cmd": "dom_snapshot", "path": "../../x" } });
        assert!(parse_request(&token(), 0, extra.to_string().as_bytes()).is_err());
    }

    #[test]
    fn oversized_messages_are_refused_before_parsing() {
        let big = vec![b' '; MAX_MESSAGE_BYTES as usize + 1];
        assert!(parse_request(&token(), 0, &big).is_err());
    }

    #[test]
    fn argument_limits_are_enforced() {
        let long_text = "x".repeat(MAX_TEXT_CHARS + 1);
        let cmd = json!({ "cmd": "type_text", "handle": 1, "text": long_text });
        assert!(Command::from_json(&cmd).is_err());
        let nul = json!({ "cmd": "type_text", "handle": 1, "text": "a\u{0}b" });
        assert!(Command::from_json(&nul).is_err());
        let role = json!({ "cmd": "find", "role": "Button/../x" });
        assert!(Command::from_json(&role).is_err());
        let handle = json!({ "cmd": "click", "handle": -1 });
        assert!(Command::from_json(&handle).is_err());
    }

    #[test]
    fn screenshot_names_cannot_leave_the_artifact_directory() {
        for bad in ["../evil", "a/b", "a\\b", "", "con.", "x y", "c:evil"] {
            let cmd = json!({ "cmd": "screenshot", "name": bad });
            assert!(Command::from_json(&cmd).is_err(), "{bad}");
        }
        assert!(is_artifact_stem("home-page_1"));
    }

    #[test]
    fn responses_round_trip_and_carry_errors() {
        let ok = encode_response(&token(), 4, &Ok(json!({ "handle": 7 })));
        assert_eq!(
            parse_response(&token(), 4, &ok).unwrap(),
            Response::Ok(json!({ "handle": 7 }))
        );
        let err = encode_response(
            &token(),
            4,
            &Err(ChannelError {
                code: "TS0303",
                message: "no such element".into(),
            }),
        );
        assert_eq!(
            parse_response(&token(), 4, &err).unwrap(),
            Response::Err {
                code: "TS0303".into(),
                message: "no such element".into()
            }
        );
        assert!(parse_response(&token(), 5, &ok).is_err());
    }

    /// A small deterministic generator for the mutation tests below.
    fn xorshift(state: &mut u64) -> u64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *state
    }

    #[test]
    fn random_and_mutated_messages_never_panic() {
        let seed_cmds = [
            Command::Find {
                role: "button".into(),
                name: Some("Compress".into()),
            },
            Command::TypeText {
                handle: 9,
                text: "héllo".into(),
            },
            Command::Eval {
                script: "1 + 1".into(),
            },
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for cmd in &seed_cmds {
            let good = encode_request(&token(), 0, cmd);
            for _ in 0..3000 {
                let mut bytes = good.clone();
                for _ in 0..=(xorshift(&mut state) % 4) {
                    let at = (xorshift(&mut state) as usize) % bytes.len();
                    bytes[at] = xorshift(&mut state) as u8;
                }
                if xorshift(&mut state).is_multiple_of(8) {
                    let cut = (xorshift(&mut state) as usize) % bytes.len();
                    bytes.truncate(cut);
                }
                let _ = parse_request(&token(), 0, &bytes);
                let _ = parse_response(&token(), 0, &bytes);
            }
        }
        // Pure noise is never a valid request: it has no token that matches this run.
        for _ in 0..3000 {
            let len = (xorshift(&mut state) % 300) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| xorshift(&mut state) as u8).collect();
            assert!(parse_request(&token(), 0, &bytes).is_err());
        }
    }

    #[test]
    fn a_file_round_trip_between_driver_and_app() {
        let base = scratch("round-trip");
        let mut driver = FileChannel::create(&base).unwrap();
        let mut app = AppChannel::open(driver.dir(), driver.token().as_str()).unwrap();
        // The app answers from another thread, as a running app would.
        let token = driver.token().clone();
        let dir = driver.dir().to_path_buf();
        let handle = std::thread::spawn(move || {
            let mut app = AppChannel::open(&dir, token.as_str()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(req) = app.poll() {
                    let req = req.unwrap();
                    let result = match req.command {
                        Command::Click { handle } => Ok(json!({ "clicked": handle })),
                        _ => Err(ChannelError {
                            code: "TS0304",
                            message: "not here".into(),
                        }),
                    };
                    app.respond(req.seq, &result).unwrap();
                    return;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let data = driver
            .call(&Command::Click { handle: 3 }, Duration::from_secs(5))
            .unwrap();
        handle.join().unwrap();
        assert_eq!(data, json!({ "clicked": 3 }));
        assert!(app.poll().is_none());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_silent_app_times_out_with_a_stable_code() {
        let base = scratch("timeout");
        let mut driver = FileChannel::create(&base).unwrap();
        let err = driver
            .call(&Command::DomSnapshot, Duration::from_millis(30))
            .unwrap_err();
        assert_eq!(err.code, "TS0302");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_run_directory_is_never_reused() {
        let base = scratch("fresh");
        let a = FileChannel::create(&base).unwrap();
        let b = FileChannel::create(&base).unwrap();
        assert_ne!(a.dir(), b.dir());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_app_refuses_a_directory_that_does_not_match_its_token() {
        let base = scratch("mismatch");
        let driver = FileChannel::create(&base).unwrap();
        let other = Token::generate();
        assert!(AppChannel::open(driver.dir(), other.as_str()).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn artifact_reads_refuse_paths_and_other_extensions() {
        let base = scratch("artifacts");
        let driver = FileChannel::create(&base).unwrap();
        let app = AppChannel::open(driver.dir(), driver.token().as_str()).unwrap();
        app.write_artifact("home", b"\x89PNG-fake").unwrap();
        assert_eq!(driver.read_artifact("home.png").unwrap(), b"\x89PNG-fake");
        for bad in ["../run.json", "home.txt", "a/home.png", "..png"] {
            assert!(driver.read_artifact(bad).is_err(), "{bad}");
        }
        assert!(app.write_artifact("../x", b"").is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn request_and_response_names_use_only_digits() {
        assert_eq!(request_file(12), "req-00000012.json");
        assert_eq!(response_file(12), "res-00000012.json");
    }
}
