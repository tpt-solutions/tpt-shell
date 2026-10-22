// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Serial ports (spec §10.2, M7). A port is opened only when its identifier is inside the
//! caller's scope. The returned transport can be given to a [`crate::hub::DeviceHub`] so that
//! its use is audited.
//!
//! Support: Partial. Windows: `COM1` to `COM999` at a given baud rate, 8 data bits, no parity, one
//! stop bit, tested. macOS and Linux: termios with every line setting in
//! [`crate::options::SerialOptions`], compile-checked only from this workspace and run on CI when a
//! port is available. Discovery lists `/dev` nodes on macOS and Linux; Windows has none.

use crate::id::{DeviceId, DeviceScope};
use crate::options::SerialOptions;
use crate::transport::{DeviceError, Transport};

/// Opens the serial port `id` at `baud` (8 data bits, no parity, one stop bit) if `scope` covers it.
///
/// Support: Partial. Windows is tested; macOS and Linux are compile-checked and run on CI only
/// when a port is present.
pub fn open(
    scope: &DeviceScope,
    id: &DeviceId,
    baud: u32,
) -> Result<Box<dyn Transport>, DeviceError> {
    if baud == 0 {
        return Err(DeviceError::Denied(
            "a baud rate of 0 is not a rate".to_string(),
        ));
    }
    open_with(
        scope,
        id,
        &SerialOptions {
            baud,
            ..SerialOptions::default()
        },
    )
}

/// Opens the serial port `id` with `options` if `scope` covers it.
///
/// Support: Partial. macOS and Linux apply every setting. Windows applies the baud rate only, and
/// refuses any other setting that differs from 8N1 with no flow control rather than ignoring it.
pub fn open_with(
    scope: &DeviceScope,
    id: &DeviceId,
    options: &SerialOptions,
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
    if options.baud == 0 {
        return Err(DeviceError::Denied(
            "a baud rate of 0 is not a rate".to_string(),
        ));
    }
    platform_open(id, port, options)
}

/// The serial ports present, or an empty list where discovery is not built (Windows).
///
/// Support: Partial. macOS and Linux list `/dev` nodes (compile-checked); Windows returns nothing.
pub fn list() -> Vec<DeviceId> {
    #[cfg(unix)]
    {
        crate::platform::unix_serial::list()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn platform_open(
    id: &DeviceId,
    port: &str,
    options: &SerialOptions,
) -> Result<Box<dyn Transport>, DeviceError> {
    let defaults = SerialOptions::default();
    if (SerialOptions {
        baud: defaults.baud,
        ..*options
    }) != defaults
    {
        return Err(DeviceError::NotSupported(
            "serial settings other than 8N1 without flow control on Windows",
        ));
    }
    let open = crate::platform::win_serial::open(id, port, options.baud)?;
    Ok(Box::new(open))
}

#[cfg(unix)]
fn platform_open(
    id: &DeviceId,
    port: &str,
    options: &SerialOptions,
) -> Result<Box<dyn Transport>, DeviceError> {
    let open = crate::platform::unix_serial::open(id, port, options)?;
    Ok(Box::new(open))
}

#[cfg(not(any(windows, unix)))]
fn platform_open(
    _id: &DeviceId,
    _port: &str,
    _options: &SerialOptions,
) -> Result<Box<dyn Transport>, DeviceError> {
    Err(DeviceError::NotSupported("serial ports on this platform"))
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

    #[cfg(unix)]
    #[test]
    fn an_absent_unix_port_is_denied_not_faked() {
        let id: DeviceId = "serial:/dev/ttyTPTNONE9".parse().expect("id");
        let scope = DeviceScope::Serial {
            port: "/dev/*".into(),
        };
        assert!(matches!(
            open(&scope, &id, 9600),
            Err(DeviceError::Denied(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_nonstandard_baud_is_refused_on_unix() {
        let id: DeviceId = "serial:/dev/ttyTPTNONE9".parse().expect("id");
        let scope = DeviceScope::Serial {
            port: "/dev/*".into(),
        };
        let options = SerialOptions {
            baud: 9601,
            ..SerialOptions::default()
        };
        assert!(matches!(
            open_with(&scope, &id, &options),
            Err(DeviceError::Denied(_))
        ));
    }
}
