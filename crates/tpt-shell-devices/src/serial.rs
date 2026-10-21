// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Serial ports (spec §10.2, M7). A port is opened only when its identifier is inside the
//! caller's scope. The returned transport can be given to a [`crate::hub::DeviceHub`] so that
//! its use is audited.
//!
//! Support: Full on Windows for `COM1` to `COM999` at a given baud rate, 8 data bits, no parity,
//! one stop bit. Port discovery is not built, so the caller names the port. macOS and Linux are
//! Unsupported for now: the termios backend is not written, and calls return
//! [`DeviceError::NotSupported`].

use crate::id::{DeviceId, DeviceScope};
use crate::transport::{DeviceError, Transport};

/// Opens the serial port `id` at `baud` if `scope` covers it.
pub fn open(
    scope: &DeviceScope,
    id: &DeviceId,
    baud: u32,
) -> Result<Box<dyn Transport>, DeviceError> {
    let port = match id {
        DeviceId::Serial { port } if scope.matches(id) => port,
        DeviceId::Serial { .. } => {
            return Err(DeviceError::Denied(format!(
                "{id} is outside the granted scope"
            )))
        }
        _ => return Err(DeviceError::Denied(format!("{id} is not a serial port"))),
    };
    if baud == 0 {
        return Err(DeviceError::Denied(
            "a baud rate of 0 is not a rate".to_string(),
        ));
    }
    platform_open(id, port, baud)
}

#[cfg(windows)]
fn platform_open(id: &DeviceId, port: &str, baud: u32) -> Result<Box<dyn Transport>, DeviceError> {
    let open = crate::platform::win_serial::open(id, port, baud)?;
    Ok(Box::new(open))
}

#[cfg(not(windows))]
fn platform_open(
    _id: &DeviceId,
    _port: &str,
    _baud: u32,
) -> Result<Box<dyn Transport>, DeviceError> {
    Err(DeviceError::NotSupported(
        "serial ports on this platform (the termios backend is not built yet)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_outside_the_scope_is_refused_before_it_is_opened() {
        let id: DeviceId = "serial:COM3".parse().expect("id");
        let scope = DeviceScope::Serial {
            port: "COM1".into(),
        };
        assert!(matches!(
            open(&scope, &id, 9600),
            Err(DeviceError::Denied(_))
        ));
    }

    #[test]
    fn a_non_serial_identifier_is_refused() {
        let id: DeviceId = "hid:1a2b:0001".parse().expect("id");
        let scope = DeviceScope::Serial { port: "*".into() };
        assert!(matches!(
            open(&scope, &id, 9600),
            Err(DeviceError::Denied(_))
        ));
    }

    #[test]
    fn a_zero_baud_rate_is_refused() {
        let id: DeviceId = "serial:COM3".parse().expect("id");
        let scope = DeviceScope::Serial {
            port: "COM*".into(),
        };
        assert!(matches!(open(&scope, &id, 0), Err(DeviceError::Denied(_))));
    }

    #[cfg(not(windows))]
    #[test]
    fn serial_is_unsupported_off_windows_until_termios_lands() {
        let id: DeviceId = "serial:/dev/ttyUSB0".parse().expect("id");
        let scope = DeviceScope::Serial {
            port: "/dev/*".into(),
        };
        assert!(matches!(
            open(&scope, &id, 9600),
            Err(DeviceError::NotSupported(_))
        ));
    }
}
