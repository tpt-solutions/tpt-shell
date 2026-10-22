// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Network devices (spec §10.2, M7): raw TCP and UDP to a printer or other device, such as a
//! printer on port 9100.
//!
//! No socket is opened unless the `net-devices` cargo feature is on. Without it, both open
//! functions return [`DeviceError::NotSupported`] before touching the network. With it, the
//! identifier must be a `net:` device inside the caller's scope.
//!
//! Support: Partial. A timeout during a write that has already sent some bytes is reported as an
//! error, not as busy, because the bytes cannot be taken back. Name resolution uses the system
//! resolver, which can block for as long as the system takes to answer.

use std::time::Duration;

use crate::id::{DeviceId, DeviceScope};
use crate::transport::{DeviceError, Transport};

/// Checks that `id` is a network device inside `scope`, and returns its host and port.
#[cfg(feature = "net-devices")]
fn target(scope: &DeviceScope, id: &DeviceId) -> Result<(String, u16), DeviceError> {
    match id {
        DeviceId::Net { host, port } if scope.matches(id) => Ok((host.clone(), *port)),
        DeviceId::Net { .. } => Err(DeviceError::Denied(format!(
            "{id} is outside the granted scope"
        ))),
        _ => Err(DeviceError::Denied(format!("{id} is not a network device"))),
    }
}

/// Opens a raw TCP connection to a network device.
///
/// Support: Partial. A timeout during a write that has already sent some bytes is an error, not
/// busy, because the bytes cannot be taken back. Name resolution uses the system resolver, which
/// can block for as long as the system takes to answer.
#[cfg(feature = "net-devices")]
pub fn open_tcp(
    scope: &DeviceScope,
    id: &DeviceId,
    timeout: Duration,
) -> Result<Box<dyn Transport>, DeviceError> {
    use std::net::TcpStream;

    let (host, port) = target(scope, id)?;
    let addr = resolve(&host, port)?;
    let stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| DeviceError::Io(format!("connect to {id} failed: {e}")))?;
    stream
        .set_nodelay(true)
        .map_err(|e| DeviceError::Io(e.to_string()))?;
    Ok(Box::new(TcpDevice {
        id: id.clone(),
        stream,
        timeout,
    }))
}

/// Opens a UDP socket connected to a network device. Each write is one datagram.
///
/// Support: Partial. A timeout during a write that has already sent some bytes is an error, not
/// busy, because the bytes cannot be taken back. Name resolution uses the system resolver, which
/// can block for as long as the system takes to answer.
#[cfg(feature = "net-devices")]
pub fn open_udp(
    scope: &DeviceScope,
    id: &DeviceId,
    timeout: Duration,
) -> Result<Box<dyn Transport>, DeviceError> {
    use std::net::UdpSocket;

    let (host, port) = target(scope, id)?;
    let addr = resolve(&host, port)?;
    let bind = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind).map_err(|e| DeviceError::Io(e.to_string()))?;
    socket
        .connect(addr)
        .map_err(|e| DeviceError::Io(format!("connect to {id} failed: {e}")))?;
    socket
        .set_write_timeout(Some(timeout))
        .map_err(|e| DeviceError::Io(e.to_string()))?;
    Ok(Box::new(UdpDevice {
        id: id.clone(),
        socket,
    }))
}

#[cfg(feature = "net-devices")]
fn resolve(host: &str, port: u16) -> Result<std::net::SocketAddr, DeviceError> {
    use std::net::ToSocketAddrs;

    (host, port)
        .to_socket_addrs()
        .map_err(|e| DeviceError::Io(format!("cannot resolve {host}: {e}")))?
        .next()
        .ok_or_else(|| DeviceError::Io(format!("{host} has no address")))
}

#[cfg(feature = "net-devices")]
struct TcpDevice {
    id: DeviceId,
    stream: std::net::TcpStream,
    timeout: Duration,
}

#[cfg(feature = "net-devices")]
impl Transport for TcpDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        use std::io::{ErrorKind, Write};

        self.stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|e| DeviceError::Io(e.to_string()))?;
        let mut sent = 0;
        while sent < bytes.len() {
            match self.stream.write(&bytes[sent..]) {
                Ok(0) => return Err(DeviceError::Closed),
                Ok(n) => sent += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return if sent == 0 {
                        Err(DeviceError::Busy)
                    } else {
                        Err(DeviceError::Io(format!(
                            "write timed out after {sent} of {} bytes",
                            bytes.len()
                        )))
                    };
                }
                Err(e)
                    if matches!(e.kind(), ErrorKind::BrokenPipe | ErrorKind::ConnectionReset) =>
                {
                    return Err(DeviceError::Closed);
                }
                Err(e) => return Err(DeviceError::Io(e.to_string())),
            }
        }
        Ok(())
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        use std::io::{ErrorKind, Read};

        if max == 0 {
            return Ok(Vec::new());
        }
        // A short read timeout: an empty result means nothing arrived yet, not an error.
        self.stream
            .set_read_timeout(Some(Duration::from_millis(1)))
            .map_err(|e| DeviceError::Io(e.to_string()))?;
        let mut buf = vec![0u8; max.min(65536)];
        match self.stream.read(&mut buf) {
            Ok(0) => Err(DeviceError::Closed),
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => Ok(Vec::new()),
            Err(e) => Err(DeviceError::Io(e.to_string())),
        }
    }
}

#[cfg(feature = "net-devices")]
struct UdpDevice {
    id: DeviceId,
    socket: std::net::UdpSocket,
}

#[cfg(feature = "net-devices")]
impl Transport for UdpDevice {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let n = self
            .socket
            .send(bytes)
            .map_err(|e| DeviceError::Io(e.to_string()))?;
        if n == bytes.len() {
            Ok(())
        } else {
            Err(DeviceError::Io(format!(
                "sent {n} of {} bytes",
                bytes.len()
            )))
        }
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        use std::io::ErrorKind;

        if max == 0 {
            return Ok(Vec::new());
        }
        self.socket
            .set_read_timeout(Some(Duration::from_millis(1)))
            .map_err(|e| DeviceError::Io(e.to_string()))?;
        let mut buf = vec![0u8; max.min(65536)];
        match self.socket.recv(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                Ok(Vec::new())
            }
            Err(e) => Err(DeviceError::Io(e.to_string())),
        }
    }
}

/// Without the `net-devices` feature no socket is opened.
///
/// Support: Unsupported. Returns NotSupported unless the net-devices feature is on, so no socket is
/// opened.
#[cfg(not(feature = "net-devices"))]
pub fn open_tcp(
    scope: &DeviceScope,
    id: &DeviceId,
    timeout: Duration,
) -> Result<Box<dyn Transport>, DeviceError> {
    let _ = (scope, id, timeout);
    Err(DeviceError::NotSupported(
        "network devices (build tpt-shell-devices with the net-devices feature)",
    ))
}

/// Without the `net-devices` feature no socket is opened.
///
/// Support: Unsupported. Returns NotSupported unless the net-devices feature is on, so no socket is
/// opened.
#[cfg(not(feature = "net-devices"))]
pub fn open_udp(
    scope: &DeviceScope,
    id: &DeviceId,
    timeout: Duration,
) -> Result<Box<dyn Transport>, DeviceError> {
    let _ = (scope, id, timeout);
    Err(DeviceError::NotSupported(
        "network devices (build tpt-shell-devices with the net-devices feature)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_for(host: &str, port: u16) -> DeviceScope {
        DeviceScope::Net {
            host: host.into(),
            port: Some(port),
        }
    }

    #[cfg(not(feature = "net-devices"))]
    #[test]
    fn without_the_feature_no_network_device_opens() {
        let id: DeviceId = "net:127.0.0.1:9100".parse().expect("id");
        let scope = scope_for("127.0.0.1", 9100);
        assert!(matches!(
            open_tcp(&scope, &id, Duration::from_secs(1)),
            Err(DeviceError::NotSupported(_))
        ));
        assert!(matches!(
            open_udp(&scope, &id, Duration::from_secs(1)),
            Err(DeviceError::NotSupported(_))
        ));
    }

    #[cfg(feature = "net-devices")]
    #[test]
    fn a_device_outside_the_scope_is_refused_before_connecting() {
        let id: DeviceId = "net:127.0.0.1:9100".parse().expect("id");
        let other = scope_for("192.0.2.1", 9100);
        assert!(matches!(
            open_tcp(&other, &id, Duration::from_millis(50)),
            Err(DeviceError::Denied(_))
        ));
        let serial: DeviceId = "serial:COM3".parse().expect("id");
        let any = DeviceScope::Net {
            host: "*".into(),
            port: None,
        };
        assert!(matches!(
            open_tcp(&any, &serial, Duration::from_millis(50)),
            Err(DeviceError::Denied(_))
        ));
    }

    #[cfg(feature = "net-devices")]
    #[test]
    fn a_tcp_device_round_trips_bytes_with_a_local_peer() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let peer = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().expect("accept");
            let mut got = [0u8; 5];
            conn.read_exact(&mut got).expect("read");
            conn.write_all(&got).expect("echo");
        });
        let id: DeviceId = format!("net:127.0.0.1:{port}").parse().expect("id");
        let mut device =
            open_tcp(&scope_for("127.0.0.1", port), &id, Duration::from_secs(5)).expect("open");
        device.write(b"hello").expect("write");
        let mut back = Vec::new();
        for _ in 0..2000 {
            back.extend(device.read(16).expect("read"));
            if back.len() >= 5 {
                break;
            }
        }
        assert_eq!(back, b"hello");
        peer.join().expect("peer");
    }
}
