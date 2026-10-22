// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! D-Bus wire format: values, messages, and their encoding and decoding (D-Bus specification,
//! "Message Protocol" and "Type System").
//!
//! This module opens no socket and needs no feature. It is pure byte handling, so it is
//! tested on every platform. The transport that moves these bytes is behind the `dbus` feature.

use std::fmt;

/// Largest message accepted, the limit the D-Bus daemon itself enforces.
///
/// Support: Full.
pub const MAX_MESSAGE: usize = 128 * 1024 * 1024;

/// A D-Bus value. `Array` keeps its element signature, so an empty array still has a type.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Bool(bool),
    Int16(i16),
    Uint16(u16),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    Str(String),
    ObjectPath(String),
    Signature(String),
    Variant(Box<Value>),
    Array(String, Vec<Value>),
    Struct(Vec<Value>),
    DictEntry(Box<Value>, Box<Value>),
}

impl Value {
    /// Support: Full.
    pub fn str(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    /// Support: Full.
    pub fn object_path(s: &str) -> Value {
        Value::ObjectPath(s.to_string())
    }

    /// Support: Full.
    pub fn variant(v: Value) -> Value {
        Value::Variant(Box::new(v))
    }

    /// An `a{sv}` dictionary from string keys and values.
    ///
    /// Support: Full.
    pub fn dict_sv(entries: Vec<(&str, Value)>) -> Value {
        Value::Array(
            "{sv}".to_string(),
            entries
                .into_iter()
                .map(|(k, v)| {
                    Value::DictEntry(Box::new(Value::str(k)), Box::new(Value::variant(v)))
                })
                .collect(),
        )
    }

    /// An `ay` byte array.
    ///
    /// Support: Full.
    pub fn bytes(data: &[u8]) -> Value {
        Value::Array(
            "y".to_string(),
            data.iter().copied().map(Value::Byte).collect(),
        )
    }

    /// The D-Bus signature of this value.
    ///
    /// Support: Full.
    pub fn signature(&self) -> String {
        let mut out = String::new();
        self.write_signature(&mut out);
        out
    }

    fn write_signature(&self, out: &mut String) {
        out.push_str(match self {
            Value::Byte(_) => "y",
            Value::Bool(_) => "b",
            Value::Int16(_) => "n",
            Value::Uint16(_) => "q",
            Value::Int32(_) => "i",
            Value::Uint32(_) => "u",
            Value::Int64(_) => "x",
            Value::Uint64(_) => "t",
            Value::Double(_) => "d",
            Value::Str(_) => "s",
            Value::ObjectPath(_) => "o",
            Value::Signature(_) => "g",
            Value::Variant(_) => "v",
            Value::Array(elem, _) => {
                out.push('a');
                out.push_str(elem);
                return;
            }
            Value::Struct(fields) => {
                out.push('(');
                for f in fields {
                    f.write_signature(out);
                }
                out.push(')');
                return;
            }
            Value::DictEntry(k, v) => {
                out.push('{');
                k.write_signature(out);
                v.write_signature(out);
                out.push('}');
                return;
            }
        });
    }

    /// Support: Full.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::ObjectPath(s) | Value::Signature(s) => Some(s),
            _ => None,
        }
    }

    /// Support: Full.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::Uint32(v) => Some(*v),
            _ => None,
        }
    }

    /// Support: Full.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Support: Full.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(_, items) => Some(items),
            _ => None,
        }
    }

    /// Support: Full.
    pub fn as_struct(&self) -> Option<&[Value]> {
        match self {
            Value::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Looks through any number of variant wrappers.
    ///
    /// Support: Full.
    pub fn unwrap_variant(&self) -> &Value {
        match self {
            Value::Variant(inner) => inner.unwrap_variant(),
            other => other,
        }
    }

    /// The bytes of an `ay` array, or `None` if the value is another type.
    ///
    /// Support: Full.
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        let items = self.as_array()?;
        items
            .iter()
            .map(|v| match v {
                Value::Byte(b) => Some(*b),
                _ => None,
            })
            .collect()
    }
}

/// Message kinds (D-Bus specification, message types).
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    MethodCall,
    MethodReturn,
    Error,
    Signal,
}

impl Kind {
    fn code(self) -> u8 {
        match self {
            Kind::MethodCall => 1,
            Kind::MethodReturn => 2,
            Kind::Error => 3,
            Kind::Signal => 4,
        }
    }

    fn from_code(code: u8) -> Option<Kind> {
        Some(match code {
            1 => Kind::MethodCall,
            2 => Kind::MethodReturn,
            3 => Kind::Error,
            4 => Kind::Signal,
            _ => return None,
        })
    }
}

/// The header fields this crate reads and writes.
///
/// Support: Full.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub reply_serial: Option<u32>,
    pub destination: Option<String>,
    pub sender: Option<String>,
}

/// One D-Bus message.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub kind: Kind,
    pub serial: u32,
    pub header: Header,
    pub body: Vec<Value>,
}

/// A malformed or unsupported message.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireError(pub String);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed D-Bus message: {}", self.0)
    }
}

impl std::error::Error for WireError {}

type WireResult<T> = std::result::Result<T, WireError>;

fn bad<T>(what: &str) -> WireResult<T> {
    Err(WireError(what.to_string()))
}

impl Message {
    /// Support: Full.
    pub fn method_call(
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Message {
        Message {
            kind: Kind::MethodCall,
            serial: 0,
            header: Header {
                path: Some(path.to_string()),
                interface: Some(interface.to_string()),
                member: Some(member.to_string()),
                destination: Some(destination.to_string()),
                ..Header::default()
            },
            body,
        }
    }

    /// A successful reply to `call`. The serial is set by the sender.
    ///
    /// Support: Full.
    pub fn method_return(call: &Message, body: Vec<Value>) -> Message {
        Message {
            kind: Kind::MethodReturn,
            serial: 0,
            header: Header {
                reply_serial: Some(call.serial),
                destination: call.header.sender.clone(),
                ..Header::default()
            },
            body,
        }
    }

    /// An error reply to `call`.
    ///
    /// Support: Full.
    pub fn error(call: &Message, name: &str, text: &str) -> Message {
        Message {
            kind: Kind::Error,
            serial: 0,
            header: Header {
                reply_serial: Some(call.serial),
                error_name: Some(name.to_string()),
                destination: call.header.sender.clone(),
                ..Header::default()
            },
            body: vec![Value::str(text)],
        }
    }

    /// The body signature, the concatenation of the value signatures.
    ///
    /// Support: Full.
    pub fn signature(&self) -> String {
        self.body.iter().map(Value::signature).collect()
    }

    /// Encodes the message in little-endian form.
    ///
    /// Support: Full.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Encoder::default();
        for v in &self.body {
            body.value(v);
        }
        let mut fields = Vec::new();
        let mut push = |code: u8, v: Value| {
            fields.push(Value::Struct(vec![Value::Byte(code), Value::variant(v)]));
        };
        if let Some(p) = &self.header.path {
            push(1, Value::ObjectPath(p.clone()));
        }
        if let Some(i) = &self.header.interface {
            push(2, Value::str(i));
        }
        if let Some(m) = &self.header.member {
            push(3, Value::str(m));
        }
        if let Some(e) = &self.header.error_name {
            push(4, Value::str(e));
        }
        if let Some(r) = self.header.reply_serial {
            push(5, Value::Uint32(r));
        }
        if let Some(d) = &self.header.destination {
            push(6, Value::str(d));
        }
        if let Some(s) = &self.header.sender {
            push(7, Value::str(s));
        }
        let sig = self.signature();
        if !sig.is_empty() {
            push(8, Value::Signature(sig));
        }
        let mut out = Encoder::default();
        out.raw(&[b'l', self.kind.code(), 0, 1]);
        out.raw(&(body.buf.len() as u32).to_le_bytes());
        out.raw(&self.serial.to_le_bytes());
        out.value(&Value::Array("(yv)".to_string(), fields));
        out.align(8);
        out.raw(&body.buf);
        out.buf
    }

    /// Decodes one complete message. `data` must hold exactly the bytes of one message, as
    /// reported by [`frame_rest`].
    ///
    /// Support: Full.
    pub fn decode(data: &[u8]) -> WireResult<Message> {
        let mut d = Decoder::new(data);
        let endian = d.u8()?;
        let little = match endian {
            b'l' => true,
            b'B' => false,
            _ => return bad("unknown endianness byte"),
        };
        d.little = little;
        let kind =
            Kind::from_code(d.u8()?).ok_or_else(|| WireError("unknown message type".into()))?;
        let _flags = d.u8()?;
        if d.u8()? != 1 {
            return bad("unsupported protocol version");
        }
        let body_len = d.u32()? as usize;
        let serial = d.u32()?;
        let fields = match d.read_type("a(yv)")? {
            Value::Array(_, items) => items,
            _ => return bad("header fields are not an array"),
        };
        let mut header = Header::default();
        let mut sig = String::new();
        for field in fields {
            let parts = field
                .as_struct()
                .ok_or_else(|| WireError("header field".into()))?;
            let code = match parts.first() {
                Some(Value::Byte(c)) => *c,
                _ => return bad("header field code"),
            };
            let value = parts
                .get(1)
                .map(Value::unwrap_variant)
                .ok_or_else(|| WireError("header field value".into()))?;
            match code {
                1 => header.path = value.as_str().map(str::to_string),
                2 => header.interface = value.as_str().map(str::to_string),
                3 => header.member = value.as_str().map(str::to_string),
                4 => header.error_name = value.as_str().map(str::to_string),
                5 => header.reply_serial = value.as_u32(),
                6 => header.destination = value.as_str().map(str::to_string),
                7 => header.sender = value.as_str().map(str::to_string),
                8 => sig = value.as_str().unwrap_or_default().to_string(),
                _ => {}
            }
        }
        d.align(8)?;
        let body_start = d.pos;
        if data.len() != body_start + body_len {
            return bad("message length does not match its header");
        }
        let mut body = Vec::new();
        let mut rest: &str = &sig;
        let mut bd = Decoder {
            data: &data[body_start..],
            pos: 0,
            little,
        };
        while !rest.is_empty() {
            let (value, next) = bd.read_type_from(rest)?;
            body.push(value);
            rest = next;
        }
        if bd.pos != bd.data.len() {
            return bad("trailing bytes after the body");
        }
        Ok(Message {
            kind,
            serial,
            header,
            body,
        })
    }
}

/// How many bytes follow the fixed 16-byte part of a message, so a reader can take one message
/// off a stream. The caller reads 16 bytes first and passes them here.
///
/// Support: Full.
pub fn frame_rest(fixed: &[u8; 16]) -> WireResult<usize> {
    let body_len = u32::from_le_bytes([fixed[4], fixed[5], fixed[6], fixed[7]]) as usize;
    let fields_len = u32::from_le_bytes([fixed[12], fixed[13], fixed[14], fixed[15]]) as usize;
    if fixed[0] != b'l' {
        return bad("big-endian messages are not accepted from the bus");
    }
    let body_start = align_up(16 + fields_len, 8);
    let total = body_start
        .checked_add(body_len)
        .ok_or_else(|| WireError("message too large".into()))?;
    if total > MAX_MESSAGE {
        return bad("message larger than the 128 MiB limit");
    }
    Ok(total - 16)
}

fn align_up(n: usize, a: usize) -> usize {
    n.div_ceil(a) * a
}

/// Alignment of a type given its first signature character.
fn alignment(first: u8) -> usize {
    match first {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' => 4,
        b'x' | b't' | b'd' | b'(' | b'{' => 8,
        _ => 1,
    }
}

/// Splits off the first complete type of `sig`. Returns (that type, the rest).
fn split_type(sig: &str) -> WireResult<(&str, &str)> {
    let bytes = sig.as_bytes();
    let first = *bytes
        .first()
        .ok_or_else(|| WireError("empty signature".into()))?;
    let len = match first {
        b'a' => {
            let (inner, _) = split_type(&sig[1..])?;
            1 + inner.len()
        }
        b'(' | b'{' => {
            let close = if first == b'(' { b')' } else { b'}' };
            let mut depth = 0usize;
            let mut end = None;
            for (i, &b) in bytes.iter().enumerate() {
                if b == first {
                    depth += 1;
                } else if b == close {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
            }
            end.ok_or_else(|| WireError("unbalanced container signature".into()))?
        }
        b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd' | b's' | b'o' | b'g'
        | b'v' => 1,
        _ => return bad("unsupported type in signature"),
    };
    Ok((&sig[..len], &sig[len..]))
}

/// Appends values to a buffer, padding each to its alignment.
#[derive(Default)]
struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn align(&mut self, n: usize) {
        while !self.buf.len().is_multiple_of(n) {
            self.buf.push(0);
        }
    }

    fn sig_bytes(&mut self, sig: &str) {
        self.buf.push(sig.len() as u8);
        self.raw(sig.as_bytes());
        self.buf.push(0);
    }

    fn string(&mut self, s: &str) {
        self.align(4);
        self.raw(&(s.len() as u32).to_le_bytes());
        self.raw(s.as_bytes());
        self.buf.push(0);
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Byte(b) => self.buf.push(*b),
            Value::Bool(b) => {
                self.align(4);
                self.raw(&(*b as u32).to_le_bytes());
            }
            Value::Int16(x) => {
                self.align(2);
                self.raw(&x.to_le_bytes());
            }
            Value::Uint16(x) => {
                self.align(2);
                self.raw(&x.to_le_bytes());
            }
            Value::Int32(x) => {
                self.align(4);
                self.raw(&x.to_le_bytes());
            }
            Value::Uint32(x) => {
                self.align(4);
                self.raw(&x.to_le_bytes());
            }
            Value::Int64(x) => {
                self.align(8);
                self.raw(&x.to_le_bytes());
            }
            Value::Uint64(x) => {
                self.align(8);
                self.raw(&x.to_le_bytes());
            }
            Value::Double(x) => {
                self.align(8);
                self.raw(&x.to_le_bytes());
            }
            Value::Str(s) | Value::ObjectPath(s) => self.string(s),
            Value::Signature(s) => self.sig_bytes(s),
            Value::Variant(inner) => {
                self.sig_bytes(&inner.signature());
                self.value(inner);
            }
            Value::Array(elem, items) => {
                self.align(4);
                let len_at = self.buf.len();
                self.raw(&[0; 4]);
                self.align(alignment(elem.as_bytes().first().copied().unwrap_or(b'y')));
                let start = self.buf.len();
                for item in items {
                    self.value(item);
                }
                let len = (self.buf.len() - start) as u32;
                self.buf[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
            }
            Value::Struct(fields) => {
                self.align(8);
                for f in fields {
                    self.value(f);
                }
            }
            Value::DictEntry(k, v) => {
                self.align(8);
                self.value(k);
                self.value(v);
            }
        }
    }
}

/// Reads values from a message body or header. Byte order is `little` unless it was set
/// otherwise by the message header.
struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    little: bool,
}

impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Decoder<'a> {
        Decoder {
            data,
            pos: 0,
            little: true,
        }
    }

    fn take(&mut self, n: usize) -> WireResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.data.len())
            .ok_or_else(|| WireError("read past the end of the message".into()))?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn align(&mut self, n: usize) -> WireResult<()> {
        let target = align_up(self.pos, n);
        self.take(target - self.pos).map(|_| ())
    }

    fn u8(&mut self) -> WireResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> WireResult<u16> {
        self.align(2)?;
        let b = self.take(2)?;
        let arr = [b[0], b[1]];
        Ok(if self.little {
            u16::from_le_bytes(arr)
        } else {
            u16::from_be_bytes(arr)
        })
    }

    fn u32(&mut self) -> WireResult<u32> {
        self.align(4)?;
        let b = self.take(4)?;
        let arr = [b[0], b[1], b[2], b[3]];
        Ok(if self.little {
            u32::from_le_bytes(arr)
        } else {
            u32::from_be_bytes(arr)
        })
    }

    fn u64(&mut self) -> WireResult<u64> {
        self.align(8)?;
        let b = self.take(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(b);
        Ok(if self.little {
            u64::from_le_bytes(arr)
        } else {
            u64::from_be_bytes(arr)
        })
    }

    fn string(&mut self) -> WireResult<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        if self.take(1)?[0] != 0 {
            return bad("string is not NUL-terminated");
        }
        std::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|_| WireError("string is not UTF-8".into()))
    }

    fn signature(&mut self) -> WireResult<String> {
        let len = self.u8()? as usize;
        let bytes = self.take(len)?;
        if self.take(1)?[0] != 0 {
            return bad("signature is not NUL-terminated");
        }
        std::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|_| WireError("signature is not UTF-8".into()))
    }

    /// Reads one value of the complete type `sig`, which must be exactly one type.
    fn read_type(&mut self, sig: &str) -> WireResult<Value> {
        let (value, rest) = self.read_type_from(sig)?;
        if !rest.is_empty() {
            return bad("signature holds more than one type");
        }
        Ok(value)
    }

    /// Reads the first type of `sig`. Returns the value and the rest of the signature.
    fn read_type_from<'s>(&mut self, sig: &'s str) -> WireResult<(Value, &'s str)> {
        let (ty, rest) = split_type(sig)?;
        let first = ty.as_bytes()[0];
        let value = match first {
            b'y' => Value::Byte(self.u8()?),
            b'b' => match self.u32()? {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                _ => return bad("boolean is neither 0 nor 1"),
            },
            b'n' => Value::Int16(self.u16()? as i16),
            b'q' => Value::Uint16(self.u16()?),
            b'i' => Value::Int32(self.u32()? as i32),
            b'u' => Value::Uint32(self.u32()?),
            b'x' => Value::Int64(self.u64()? as i64),
            b't' => Value::Uint64(self.u64()?),
            b'd' => Value::Double(f64::from_bits(self.u64()?)),
            b's' => Value::Str(self.string()?),
            b'o' => Value::ObjectPath(self.string()?),
            b'g' => Value::Signature(self.signature()?),
            b'v' => {
                let inner_sig = self.signature()?;
                let (inner_ty, after) = split_type(&inner_sig)?;
                if !after.is_empty() {
                    return bad("variant holds more than one type");
                }
                Value::Variant(Box::new(self.read_type(inner_ty)?))
            }
            b'a' => {
                let len = self.u32()? as usize;
                self.align(alignment(ty.as_bytes()[1]))?;
                let end = self
                    .pos
                    .checked_add(len)
                    .ok_or_else(|| WireError("array too long".into()))?;
                let mut items = Vec::new();
                while self.pos < end {
                    items.push(self.read_type(&ty[1..])?);
                }
                if self.pos != end {
                    return bad("array elements overrun the array length");
                }
                Value::Array(ty[1..].to_string(), items)
            }
            b'(' => {
                self.align(8)?;
                let inner = &ty[1..ty.len() - 1];
                let mut fields = Vec::new();
                let mut s = inner;
                while !s.is_empty() {
                    let (v, next) = self.read_type_from(s)?;
                    fields.push(v);
                    s = next;
                }
                Value::Struct(fields)
            }
            b'{' => {
                self.align(8)?;
                let inner = &ty[1..ty.len() - 1];
                let (k, after_k) = self.read_type_from(inner)?;
                let (v, after_v) = self.read_type_from(after_k)?;
                if !after_v.is_empty() {
                    return bad("dictionary entry holds more than two types");
                }
                Value::DictEntry(Box::new(k), Box::new(v))
            }
            _ => return bad("unsupported type"),
        };
        Ok((value, rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(values: Vec<Value>) -> Message {
        let mut msg = Message::method_call("org.example", "/x", "org.example.If", "Do", values);
        msg.serial = 7;
        msg.header.sender = Some(":1.2".into());
        let bytes = msg.encode();
        assert_eq!(
            frame_rest(bytes[..16].try_into().unwrap()).unwrap(),
            bytes.len() - 16
        );
        Message::decode(&bytes).expect("decodes")
    }

    #[test]
    fn scalars_strings_and_containers_round_trip() {
        let values = vec![
            Value::Byte(9),
            Value::Bool(true),
            Value::Int16(-3),
            Value::Uint16(65000),
            Value::Int32(-70000),
            Value::Uint32(4_000_000_000),
            Value::Int64(-5_000_000_000),
            Value::Uint64(u64::MAX),
            Value::Double(1.5),
            Value::str("héllo"),
            Value::object_path("/org/freedesktop/secrets"),
            Value::Signature("a{sv}".into()),
            Value::variant(Value::Uint32(42)),
            Value::Array("s".into(), vec![Value::str("a"), Value::str("bb")]),
            Value::Struct(vec![Value::Byte(1), Value::str("in struct")]),
            Value::dict_sv(vec![("urgency", Value::Byte(1))]),
            Value::bytes(&[1, 2, 3]),
        ];
        let msg = round_trip(values.clone());
        assert_eq!(msg.body, values);
        assert_eq!(msg.kind, Kind::MethodCall);
        assert_eq!(msg.serial, 7);
        assert_eq!(msg.header.member.as_deref(), Some("Do"));
        assert_eq!(msg.header.destination.as_deref(), Some("org.example"));
        assert_eq!(msg.header.sender.as_deref(), Some(":1.2"));
    }

    #[test]
    fn empty_array_keeps_its_element_type() {
        let msg = round_trip(vec![Value::Array("{sv}".into(), vec![])]);
        assert_eq!(msg.body, vec![Value::Array("{sv}".into(), vec![])]);
        assert_eq!(msg.signature(), "a{sv}");
    }

    #[test]
    fn signatures_are_built_from_values() {
        assert_eq!(
            Value::Struct(vec![Value::Byte(0), Value::str("")]).signature(),
            "(ys)"
        );
        assert_eq!(Value::dict_sv(vec![]).signature(), "a{sv}");
        assert_eq!(Value::bytes(&[]).signature(), "ay");
    }

    #[test]
    fn a_reply_names_the_call_it_answers() {
        let mut call = Message::method_call("svc", "/p", "i.f", "M", vec![]);
        call.serial = 11;
        call.header.sender = Some(":1.5".into());
        let mut reply = Message::method_return(&call, vec![Value::Uint32(3)]);
        reply.serial = 12;
        let back = Message::decode(&reply.encode()).unwrap();
        assert_eq!(back.kind, Kind::MethodReturn);
        assert_eq!(back.header.reply_serial, Some(11));
        assert_eq!(back.header.destination.as_deref(), Some(":1.5"));
        assert_eq!(back.body, vec![Value::Uint32(3)]);
    }

    #[test]
    fn an_error_reply_carries_its_name_and_text() {
        let mut call = Message::method_call("svc", "/p", "i.f", "M", vec![]);
        call.serial = 2;
        let err = Message::error(&call, "org.freedesktop.DBus.Error.Failed", "no");
        let back = Message::decode(&err.encode()).unwrap();
        assert_eq!(back.kind, Kind::Error);
        assert_eq!(
            back.header.error_name.as_deref(),
            Some("org.freedesktop.DBus.Error.Failed")
        );
        assert_eq!(back.body, vec![Value::str("no")]);
    }

    #[test]
    fn truncated_and_corrupt_input_is_refused() {
        let msg = round_trip(vec![Value::str("abc")]);
        let mut bytes = msg.encode();
        assert!(Message::decode(&bytes[..bytes.len() - 1]).is_err());
        bytes.push(0);
        assert!(Message::decode(&bytes).is_err(), "extra bytes are refused");
        let mut bad_endian = msg.encode();
        bad_endian[0] = b'x';
        assert!(Message::decode(&bad_endian).is_err());
    }

    #[test]
    fn a_boolean_other_than_0_or_1_is_refused() {
        let mut msg = Message::method_call("s", "/p", "i", "m", vec![Value::Bool(true)]);
        msg.serial = 1;
        let mut bytes = msg.encode();
        let body_start = bytes.len() - 4;
        bytes[body_start] = 2;
        assert!(Message::decode(&bytes).is_err());
    }

    #[test]
    fn oversized_frames_are_refused_before_reading() {
        let mut fixed = [0u8; 16];
        fixed[0] = b'l';
        fixed[4..8].copy_from_slice(&(MAX_MESSAGE as u32).to_le_bytes());
        assert!(frame_rest(&fixed).is_err());
        fixed[4..8].copy_from_slice(&1u32.to_le_bytes());
        fixed[0] = b'B';
        assert!(frame_rest(&fixed).is_err(), "big-endian frames are refused");
    }
}
