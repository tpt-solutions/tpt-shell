// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A D-Bus connection: EXTERNAL authentication, method calls and replies.
//!
//! [`Connection`] is generic over its stream so the protocol runs against an in-memory bus in
//! tests. The real socket is [`Stream`], which exists only with the `dbus` feature on a UNIX
//! platform. With the feature off, `Stream` is [`Closed`], every read and write fails with
//! `Unsupported`, and no socket is ever created.

use std::io::{self, Read, Write};
use std::time::Duration;

use tpt_shell_core::{Error, Result};

use super::wire::{frame_rest, Kind, Message, Value};

/// How long a call waits for its reply before it fails.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(25);

/// The bus name and path of the D-Bus daemon itself.
const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// A stream that can carry messages and can time out.
pub trait Transport: Read + Write {
    /// Sets how long a read may block. `None` waits forever.
    fn set_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
}

/// The bus socket. Only exists with the `dbus` feature on a UNIX platform.
#[cfg(all(feature = "dbus", unix))]
pub type Stream = std::os::unix::net::UnixStream;

/// Stands in for the socket when D-Bus is not built in. It never opens anything.
#[cfg(not(all(feature = "dbus", unix)))]
pub type Stream = Closed;

/// A stream that always fails with `Unsupported`.
#[derive(Debug)]
pub struct Closed;

fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "D-Bus is not built in: enable the `dbus` feature of tpt-shell-platform-linux",
    )
}

impl Read for Closed {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(closed())
    }
}

impl Write for Closed {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(closed())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(closed())
    }
}

impl Transport for Closed {
    fn set_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
        Err(closed())
    }
}

#[cfg(all(feature = "dbus", unix))]
impl Transport for std::os::unix::net::UnixStream {
    fn set_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)?;
        self.set_write_timeout(timeout)
    }
}

/// An authenticated connection to a bus.
pub struct Connection<S: Transport> {
    stream: S,
    next_serial: u32,
    unique_name: String,
}

fn backend(msg: impl Into<String>) -> Error {
    Error::Backend(msg.into())
}

impl<S: Transport> Connection<S> {
    /// Runs the EXTERNAL handshake as `uid` (decimal text, such as `1000`), then says Hello to
    /// get the connection's unique name.
    pub fn authenticate(mut stream: S, uid: &str) -> Result<Connection<S>> {
        stream
            .set_timeout(Some(CALL_TIMEOUT))
            .map_err(|e| backend(format!("bus socket: {e}")))?;
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        let mut out = b"\0".to_vec();
        out.extend_from_slice(format!("AUTH EXTERNAL {hex}\r\n").as_bytes());
        stream
            .write_all(&out)
            .map_err(|e| backend(format!("bus handshake: {e}")))?;
        let line = read_line(&mut stream)?;
        if !line.starts_with("OK ") {
            return Err(backend("the bus refused EXTERNAL authentication"));
        }
        stream
            .write_all(b"BEGIN\r\n")
            .map_err(|e| backend(format!("bus handshake: {e}")))?;
        let mut conn = Connection {
            stream,
            next_serial: 1,
            unique_name: String::new(),
        };
        let reply = conn.call(BUS_NAME, BUS_PATH, BUS_NAME, "Hello", vec![])?;
        conn.unique_name = reply
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| backend("the bus gave no unique name"))?
            .to_string();
        Ok(conn)
    }

    /// The connection's unique name, such as `:1.42`.
    pub fn unique_name(&self) -> &str {
        &self.unique_name
    }

    #[cfg(test)]
    pub(crate) fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// Sends a message, giving it the next serial. Returns that serial.
    pub fn send(&mut self, msg: &mut Message) -> Result<u32> {
        msg.serial = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1).max(1);
        let bytes = msg.encode();
        self.stream
            .write_all(&bytes)
            .and_then(|()| self.stream.flush())
            .map_err(|e| backend(format!("bus write: {e}")))?;
        Ok(msg.serial)
    }

    /// Sets how long a read may block before it fails (or, for [`Self::poll_message`], returns
    /// no message). `None` waits forever.
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<()> {
        self.stream
            .set_timeout(timeout)
            .map_err(|e| backend(format!("bus socket: {e}")))
    }

    /// Reads the next message from the bus. A read that times out is an error.
    pub fn read_message(&mut self) -> Result<Message> {
        match self.read_frame(false)? {
            Some(msg) => Ok(msg),
            None => Err(backend("the D-Bus call timed out")),
        }
    }

    /// Reads the next message if one arrives before the read timeout. `None` when the timeout
    /// passes with nothing received. Once the first byte of a message has arrived, the reader
    /// keeps waiting for the rest, so a message is never split.
    pub fn poll_message(&mut self) -> Result<Option<Message>> {
        self.read_frame(true)
    }

    fn read_frame(&mut self, idle_ok: bool) -> Result<Option<Message>> {
        let mut fixed = [0u8; 16];
        let mut got = 0;
        while got < 16 {
            match self.stream.read(&mut fixed[got..]) {
                Ok(0) => return Err(backend("the bus closed the connection")),
                Ok(n) => got += n,
                Err(e) if is_timeout(&e) => {
                    if got == 0 && idle_ok {
                        return Ok(None);
                    }
                    if !idle_ok {
                        return Err(read_error(e));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(read_error(e)),
            }
        }
        let rest = frame_rest(&fixed).map_err(|e| backend(e.to_string()))?;
        let mut bytes = vec![0u8; 16 + rest];
        bytes[..16].copy_from_slice(&fixed);
        let mut filled = 16;
        while filled < bytes.len() {
            match self.stream.read(&mut bytes[filled..]) {
                Ok(0) => return Err(backend("the bus closed the connection")),
                Ok(n) => filled += n,
                Err(e) if is_timeout(&e) && idle_ok => {}
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(read_error(e)),
            }
        }
        Message::decode(&bytes)
            .map(Some)
            .map_err(|e| backend(e.to_string()))
    }

    /// Calls a method and returns its reply body. Signals and unrelated messages that arrive
    /// meanwhile are dropped. An incoming method call is refused, so its caller is not left
    /// waiting.
    pub fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Result<Vec<Value>> {
        let mut msg = Message::method_call(destination, path, interface, member, body);
        let serial = self.send(&mut msg)?;
        loop {
            let incoming = self.read_message()?;
            if incoming.header.reply_serial != Some(serial) {
                if incoming.kind == Kind::MethodCall {
                    let mut refusal = Message::error(
                        &incoming,
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        "this client does not serve calls on this connection",
                    );
                    self.send(&mut refusal)?;
                }
                continue;
            }
            return match incoming.kind {
                Kind::MethodReturn => Ok(incoming.body),
                Kind::Error => {
                    let name = incoming.header.error_name.unwrap_or_default();
                    let text = incoming
                        .body
                        .first()
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    Err(backend(format!(
                        "{destination} {member} failed: {name}: {text}"
                    )))
                }
                _ => Err(backend("the bus sent an unexpected reply")),
            };
        }
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn read_error(e: io::Error) -> Error {
    match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => backend("the D-Bus call timed out"),
        io::ErrorKind::UnexpectedEof => backend("the bus closed the connection"),
        _ => backend(format!("bus read: {e}")),
    }
}

/// Reads one CRLF-terminated line, one byte at a time, so no message bytes are consumed early.
fn read_line<S: Read>(stream: &mut S) -> Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream
            .read_exact(&mut byte)
            .map_err(|e| backend(format!("bus handshake: {e}")))?;
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
        if line.len() > 4096 {
            return Err(backend("the bus sent an overlong handshake line"));
        }
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| backend("the bus handshake is not text"))
}

/// The session bus address the environment names. `DBUS_SESSION_BUS_ADDRESS` wins; otherwise
/// the standard socket under `XDG_RUNTIME_DIR`.
pub fn session_address() -> Option<String> {
    if let Some(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .filter(|a| !a.is_empty())
    {
        return Some(addr);
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|d| !d.is_empty())?;
    Some(format!("unix:path={runtime}/bus"))
}

/// Extracts the socket path from a `unix:path=...` address. Abstract sockets and TCP addresses
/// are refused: the shell only connects to a file-system socket it can name.
pub fn socket_path(address: &str) -> Result<String> {
    let rest = address
        .strip_prefix("unix:")
        .ok_or_else(|| backend("the session bus address is not a unix: address"))?;
    for part in rest.split(',') {
        if let Some(path) = part.strip_prefix("path=") {
            if path.is_empty() {
                break;
            }
            return Ok(path.to_string());
        }
    }
    Err(backend("the session bus address names no socket path"))
}

/// Connects to the session bus. Refused with `Unsupported` unless the `dbus` feature is on.
pub fn session() -> Result<Connection<Stream>> {
    #[cfg(all(feature = "dbus", unix))]
    {
        let address = session_address().ok_or_else(|| backend("no session bus address is set"))?;
        connect_address(&address)
    }
    #[cfg(not(all(feature = "dbus", unix)))]
    {
        Err(Error::Unsupported {
            feature: "D-Bus services (build tpt-shell-platform-linux with the dbus feature)",
        })
    }
}

/// Connects to the bus at `address` (a `unix:path=` address) and authenticates. Refused with
/// `Unsupported` unless the `dbus` feature is on.
pub fn connect_address(address: &str) -> Result<Connection<Stream>> {
    #[cfg(all(feature = "dbus", unix))]
    {
        let path = socket_path(address)?;
        let stream = std::os::unix::net::UnixStream::connect(&path)
            .map_err(|e| backend(format!("cannot connect to the bus at {path}: {e}")))?;
        let uid = current_uid()?;
        Connection::authenticate(stream, &uid)
    }
    #[cfg(not(all(feature = "dbus", unix)))]
    {
        let _ = address;
        Err(Error::Unsupported {
            feature: "D-Bus services (build tpt-shell-platform-linux with the dbus feature)",
        })
    }
}

/// The real user id, as decimal text, read from `/proc/self/status`.
#[cfg(all(feature = "dbus", target_os = "linux"))]
fn current_uid() -> Result<String> {
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|e| backend(format!("cannot read /proc/self/status: {e}")))?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
        .ok_or_else(|| backend("no Uid line in /proc/self/status"))
}

#[cfg(all(feature = "dbus", unix, not(target_os = "linux")))]
fn current_uid() -> Result<String> {
    Err(Error::Unsupported {
        feature: "D-Bus EXTERNAL authentication outside Linux",
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    //! An in-memory bus: it answers from a script and records what it was sent.

    use std::collections::VecDeque;
    use std::io::{self, Read, Write};
    use std::time::Duration;

    use super::Transport;
    use crate::dbus::wire::Message;

    pub struct ScriptedBus {
        pub input: VecDeque<u8>,
        pub written: Vec<u8>,
    }

    impl ScriptedBus {
        /// A bus that answers the handshake, then carries `messages` in order.
        pub fn new(messages: &[Message]) -> ScriptedBus {
            let mut input: VecDeque<u8> = b"OK 0123456789abcdef\r\n".iter().copied().collect();
            for m in messages {
                input.extend(m.encode());
            }
            ScriptedBus {
                input,
                written: Vec::new(),
            }
        }
    }

    impl Read for ScriptedBus {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.input.len());
            for slot in buf.iter_mut().take(n) {
                *slot = self.input.pop_front().unwrap_or(0);
            }
            Ok(n)
        }
    }

    impl Write for ScriptedBus {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Transport for ScriptedBus {
        fn set_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::ScriptedBus;
    use super::*;

    /// A reply to the Hello call (serial 1), carrying the unique name.
    fn hello_reply(name: &str) -> Message {
        let mut call = Message::method_call(BUS_NAME, BUS_PATH, BUS_NAME, "Hello", vec![]);
        call.serial = 1;
        call.header.sender = Some(BUS_NAME.into());
        Message::method_return(&call, vec![Value::str(name)])
    }

    fn reply_to(serial: u32, body: Vec<Value>) -> Message {
        let mut call = Message::method_call("x", "/", "i", "m", vec![]);
        call.serial = serial;
        Message::method_return(&call, body)
    }

    #[test]
    fn authenticate_sends_external_with_the_uid_in_hex_then_begin() {
        let bus = ScriptedBus::new(&[hello_reply(":1.42")]);
        let conn = Connection::authenticate(bus, "1000").unwrap();
        assert_eq!(conn.unique_name(), ":1.42");
        let sent = String::from_utf8_lossy(&conn.stream.written).to_string();
        assert!(
            sent.starts_with("\0AUTH EXTERNAL 31303030\r\nBEGIN\r\n"),
            "{sent:?}"
        );
    }

    #[test]
    fn a_refused_handshake_is_an_error() {
        let mut bus = ScriptedBus::new(&[]);
        bus.input = b"REJECTED EXTERNAL\r\n".iter().copied().collect();
        assert!(Connection::authenticate(bus, "1000").is_err());
    }

    #[test]
    fn call_returns_the_body_of_the_matching_reply_and_skips_signals() {
        let mut signal = Message::method_call("x", "/", "i", "Changed", vec![]);
        signal.kind = Kind::Signal;
        signal.serial = 50;
        let bus = ScriptedBus::new(&[
            hello_reply(":1.1"),
            signal,
            reply_to(2, vec![Value::Uint32(9)]),
        ]);
        let mut conn = Connection::authenticate(bus, "1000").unwrap();
        let body = conn.call("svc", "/p", "i", "m", vec![]).unwrap();
        assert_eq!(body, vec![Value::Uint32(9)]);
    }

    #[test]
    fn an_error_reply_becomes_an_error_with_the_name() {
        let mut call = Message::method_call("x", "/", "i", "m", vec![]);
        call.serial = 2;
        let err = Message::error(&call, "org.freedesktop.Secret.Error.IsLocked", "locked");
        let bus = ScriptedBus::new(&[hello_reply(":1.1"), err]);
        let mut conn = Connection::authenticate(bus, "1000").unwrap();
        let text = conn
            .call("svc", "/p", "i", "m", vec![])
            .unwrap_err()
            .to_string();
        assert!(text.contains("IsLocked"), "{text}");
    }

    #[test]
    fn a_closed_stream_fails_cleanly() {
        let mut closed = Closed;
        let mut buf = [0u8; 4];
        assert_eq!(
            closed.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(closed.write(b"x").is_err());
    }

    #[test]
    fn session_address_rejects_tcp_and_abstract_forms() {
        assert_eq!(
            socket_path("unix:path=/run/user/1000/bus").unwrap(),
            "/run/user/1000/bus"
        );
        assert!(
            socket_path("unix:abstract=/tmp/x,guid=abc").is_err(),
            "abstract sockets are refused"
        );
        assert!(socket_path("tcp:host=localhost,port=1").is_err());
        assert!(socket_path("unix:path=,guid=1").is_err());
    }

    #[cfg(not(all(feature = "dbus", unix)))]
    #[test]
    fn the_session_bus_is_unsupported_without_the_feature() {
        assert!(matches!(session(), Err(Error::Unsupported { .. })));
    }
}
