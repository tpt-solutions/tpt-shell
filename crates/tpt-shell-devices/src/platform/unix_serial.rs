// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Serial ports on macOS and Linux through termios (spec §10.2, M7).
//!
//! The port is opened non-blocking and without becoming the controlling terminal, locked with
//! `flock` so a second open of the same port is refused, and put in raw mode with the line
//! settings from [`crate::options::TermiosPlan`]. A node that is not a terminal is refused, so a
//! scope that names `/dev/*` cannot be used to read an ordinary file. Reads never wait. A write
//! that cannot take any bytes returns [`DeviceError::Busy`]; one that stalls after taking some
//! waits up to one second and then fails.
//!
//! Support: Partial. Compile-checked for Linux and macOS from a Windows host; the runtime test
//! runs only on a machine with a port named by `TPT_TEST_SERIAL_PORT`.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;

use crate::descriptors::is_serial_node;
use crate::id::DeviceId;
use crate::options::{termios_plan, SerialOptions, TermiosPlan};
use crate::transport::{DeviceError, Transport};

/// Longest a stalled write waits for the port to drain, in milliseconds.
const STALL_MS: i32 = 1000;
const CHUNK: usize = 4096;

/// An open serial port. Dropping it closes the file, which also releases the lock.
///
/// Support: Partial. Compile-checked; run only where a port is available.
pub struct UnixSerial {
    id: DeviceId,
    file: File,
}

/// The `libc` speed constant for a standard baud rate.
fn speed_constant(baud: u32) -> Option<libc::speed_t> {
    Some(match baud {
        50 => libc::B50,
        75 => libc::B75,
        110 => libc::B110,
        134 => libc::B134,
        150 => libc::B150,
        200 => libc::B200,
        300 => libc::B300,
        600 => libc::B600,
        1200 => libc::B1200,
        1800 => libc::B1800,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        _ => return None,
    })
}

/// Applies `plan` to `t` on top of raw mode. Pure over the struct, so it is tested without a port.
fn apply_plan(t: &mut libc::termios, plan: &TermiosPlan) -> Result<(), DeviceError> {
    let speed = speed_constant(plan.baud)
        .ok_or_else(|| DeviceError::Denied(format!("baud rate {} is not standard", plan.baud)))?;
    // SAFETY: `t` is a valid, exclusively borrowed termios. cfmakeraw and cfsetspeed only write
    // its fields.
    let ok = unsafe {
        libc::cfmakeraw(t);
        libc::cfsetspeed(t, speed)
    };
    if ok != 0 {
        return Err(DeviceError::Io("cannot set the baud rate".to_string()));
    }
    t.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::PARODD | libc::CSTOPB | libc::CRTSCTS);
    t.c_cflag |= match plan.char_bits {
        5 => libc::CS5,
        6 => libc::CS6,
        7 => libc::CS7,
        _ => libc::CS8,
    };
    if plan.parity_enable {
        t.c_cflag |= libc::PARENB;
    }
    if plan.parity_odd {
        t.c_cflag |= libc::PARODD;
    }
    if plan.two_stop_bits {
        t.c_cflag |= libc::CSTOPB;
    }
    if plan.hardware_flow {
        t.c_cflag |= libc::CRTSCTS;
    }
    // Receiver on, ignore modem control lines (unless hardware flow control needs them).
    t.c_cflag |= libc::CREAD | libc::CLOCAL;
    t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
    if plan.software_flow {
        t.c_iflag |= libc::IXON | libc::IXOFF;
    }
    // A read returns at once with what has arrived, possibly nothing.
    t.c_cc[libc::VMIN] = 0;
    t.c_cc[libc::VTIME] = 0;
    Ok(())
}

/// The device node for a port name: a bare name is looked up under `/dev`.
fn node_path(port: &str) -> String {
    if port.starts_with('/') {
        port.to_string()
    } else {
        format!("/dev/{port}")
    }
}

/// Opens `port` with `options`.
///
/// Support: Partial. Compile-checked; run only where a port is available.
pub fn open(id: &DeviceId, port: &str, options: &SerialOptions) -> Result<UnixSerial, DeviceError> {
    let plan = termios_plan(options)?;
    let path = node_path(port);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => DeviceError::Denied(format!("{port} is not present")),
            io::ErrorKind::PermissionDenied => DeviceError::Denied(format!(
                "no permission to open {port} (on Linux, join the group that owns the node, often dialout or uucp)"
            )),
            _ => DeviceError::Io(format!("cannot open {port}: {e}")),
        })?;
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is open for the life of `file`. flock only takes the descriptor and flags.
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(DeviceError::Denied(format!(
            "{port} is already open elsewhere"
        )));
    }
    // SAFETY: termios is plain data, and an all-zero value is a valid starting point that
    // tcgetattr overwrites.
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is open and `t` is a valid termios to fill.
    if unsafe { libc::tcgetattr(fd, &mut t) } != 0 {
        return Err(DeviceError::Denied(format!("{port} is not a terminal")));
    }
    apply_plan(&mut t, &plan)?;
    // SAFETY: `fd` is open and `t` was filled by tcgetattr and changed in documented fields.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) } != 0 {
        return Err(DeviceError::Io(format!(
            "cannot set the settings of {port}: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(UnixSerial {
        id: id.clone(),
        file,
    })
}

/// The serial ports under `/dev` that look like real ports. Nothing is opened.
///
/// Support: Partial. Compile-checked; run only where ports exist.
pub fn list() -> Vec<DeviceId> {
    let Ok(entries) = std::fs::read_dir("/dev") else {
        return Vec::new();
    };
    let mut ids: Vec<DeviceId> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| is_serial_node(name))
        .map(|name| DeviceId::Serial {
            port: format!("/dev/{name}"),
        })
        .collect();
    ids.sort_by_key(|id| id.to_string());
    ids
}

impl Transport for UnixSerial {
    fn id(&self) -> &DeviceId {
        &self.id
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError> {
        let mut sent = 0;
        while sent < bytes.len() {
            let end = bytes.len().min(sent + CHUNK);
            match self.file.write(&bytes[sent..end]) {
                Ok(0) => return Err(DeviceError::Closed),
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if sent == 0 {
                        return Err(DeviceError::Busy);
                    }
                    if !self.wait_writable() {
                        return Err(DeviceError::Io(format!(
                            "serial write timed out after {sent} of {} bytes",
                            bytes.len()
                        )));
                    }
                }
                Err(e) => return Err(DeviceError::Io(format!("cannot write to the port: {e}"))),
            }
        }
        Ok(())
    }

    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError> {
        let mut buf = vec![0u8; max.min(CHUNK)];
        match self.file.read(&mut buf) {
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(Vec::new())
            }
            Err(e) => Err(DeviceError::Io(format!("cannot read from the port: {e}"))),
        }
    }
}

impl UnixSerial {
    /// Waits up to [`STALL_MS`] for the port to take more data.
    fn wait_writable(&self) -> bool {
        let mut pfd = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        // SAFETY: `pfd` is one valid pollfd and the count passed is 1.
        let n = unsafe { libc::poll(&mut pfd, 1, STALL_MS) };
        n > 0 && pfd.revents & libc::POLLOUT != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::{DataBits, FlowControl, Parity, StopBits};

    #[test]
    fn every_standard_baud_has_a_constant() {
        for baud in crate::options::STANDARD_BAUDS {
            assert!(speed_constant(baud).is_some(), "{baud}");
        }
        assert!(speed_constant(9601).is_none());
    }

    #[test]
    fn the_plan_sets_the_control_flags() {
        let options = SerialOptions {
            baud: 19200,
            data_bits: DataBits::Seven,
            parity: Parity::Even,
            stop_bits: StopBits::Two,
            flow: FlowControl::Hardware,
        };
        let plan = termios_plan(&options).expect("plan");
        // SAFETY: an all-zero termios is valid plain data.
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        apply_plan(&mut t, &plan).expect("applied");
        assert_eq!(t.c_cflag & libc::CSIZE, libc::CS7);
        assert_ne!(t.c_cflag & libc::PARENB, 0);
        assert_eq!(t.c_cflag & libc::PARODD, 0);
        assert_ne!(t.c_cflag & libc::CSTOPB, 0);
        assert_ne!(t.c_cflag & libc::CRTSCTS, 0);
        assert_eq!(t.c_iflag & (libc::IXON | libc::IXOFF), 0);
        assert_eq!(t.c_cc[libc::VMIN], 0);
    }

    #[test]
    fn a_node_that_is_not_present_is_denied() {
        let id: DeviceId = "serial:/dev/ttyTPTNONE9".parse().expect("id");
        let result = open(&id, "/dev/ttyTPTNONE9", &SerialOptions::default());
        assert!(matches!(result, Err(DeviceError::Denied(_))));
    }

    #[test]
    fn an_ordinary_file_is_not_a_terminal() {
        // /dev/null opens but is not a terminal, so the open must be refused.
        let id: DeviceId = "serial:/dev/null".parse().expect("id");
        let result = open(&id, "/dev/null", &SerialOptions::default());
        assert!(matches!(result, Err(DeviceError::Denied(_))), "{:?}", {
            result.map(|_| ())
        });
    }

    #[test]
    fn a_named_port_opens_when_the_tester_gives_one() {
        let Ok(port) = std::env::var("TPT_TEST_SERIAL_PORT") else {
            eprintln!("skipped: set TPT_TEST_SERIAL_PORT to a free port such as /dev/ttyUSB0");
            return;
        };
        let id: DeviceId = format!("serial:{port}").parse().expect("id");
        let mut device = open(&id, &port, &SerialOptions::default()).expect("open the port");
        device.write(&[]).expect("an empty write succeeds");
        assert!(device.read(16).expect("read").len() <= 16);
    }

    #[test]
    fn listing_does_not_fail_without_ports() {
        let ids = list();
        eprintln!("serial ports seen: {}", ids.len());
    }
}
