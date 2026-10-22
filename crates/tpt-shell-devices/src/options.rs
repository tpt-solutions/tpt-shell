// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Options for opening a device (spec §10.2, M7), and the pure computation that turns serial
//! options into termios settings. Nothing here touches the operating system, so the tests run on
//! every host. The termios backend in `platform::unix_serial` maps the result onto the
//! platform's flag constants.

use crate::transport::DeviceError;

/// Data bits per character.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

/// Parity.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    None,
    Even,
    Odd,
}

/// Stop bits.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

/// Flow control.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowControl {
    None,
    /// RTS/CTS.
    Hardware,
    /// XON/XOFF.
    Software,
}

/// Serial line settings. The default is 9600 baud, 8 data bits, no parity, one stop bit, no flow
/// control.
///
/// Support: Partial. Windows applies the baud rate only and always uses 8N1 with DTR and RTS on;
/// termios (macOS, Linux) applies every field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialOptions {
    pub baud: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow: FlowControl,
}

impl Default for SerialOptions {
    fn default() -> Self {
        SerialOptions {
            baud: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow: FlowControl::None,
        }
    }
}

/// Baud rates termios has a standard constant for on both Linux and macOS.
pub const STANDARD_BAUDS: [u32; 18] = [
    50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400, 57600,
    115200, 230400,
];

impl SerialOptions {
    /// Checks the options. A baud rate that is not a standard one is refused, because no
    /// arbitrary-rate path is built.
    ///
    /// Support: Full.
    pub fn validate(&self) -> Result<(), DeviceError> {
        if !STANDARD_BAUDS.contains(&self.baud) {
            return Err(DeviceError::Denied(format!(
                "baud rate {} is not a standard rate",
                self.baud
            )));
        }
        Ok(())
    }
}

/// The settings termios needs, as booleans, before they are mapped onto the platform's flags.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermiosPlan {
    pub baud: u32,
    /// Character size in bits: 5 to 8.
    pub char_bits: u8,
    pub parity_enable: bool,
    pub parity_odd: bool,
    pub two_stop_bits: bool,
    /// CRTSCTS.
    pub hardware_flow: bool,
    /// IXON and IXOFF.
    pub software_flow: bool,
}

/// Computes the termios settings for `options`. Raw mode (no echo, no line editing, no output
/// processing, no signals) is always applied by the backend; this covers the line settings.
///
/// Support: Full.
pub fn termios_plan(options: &SerialOptions) -> Result<TermiosPlan, DeviceError> {
    options.validate()?;
    Ok(TermiosPlan {
        baud: options.baud,
        char_bits: match options.data_bits {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        },
        parity_enable: options.parity != Parity::None,
        parity_odd: options.parity == Parity::Odd,
        two_stop_bits: options.stop_bits == StopBits::Two,
        hardware_flow: options.flow == FlowControl::Hardware,
        software_flow: options.flow == FlowControl::Software,
    })
}

/// Options for [`crate::hub::DeviceHub::open_with`].
///
/// Support: Partial. Only the fields a transport understands are used; the rest are ignored by
/// transports that have no use for them (the virtual devices ignore all of them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenOptions {
    /// Serial line settings, for serial devices.
    pub serial: SerialOptions,
    /// The USB interface to claim, for raw USB devices.
    pub usb_interface: u8,
    /// How long one USB transfer may take, in milliseconds.
    pub timeout_ms: u32,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            serial: SerialOptions::default(),
            usb_interface: 0,
            timeout_ms: 1000,
        }
    }
}

impl OpenOptions {
    /// Checks every field.
    ///
    /// Support: Full.
    pub fn validate(&self) -> Result<(), DeviceError> {
        self.serial.validate()?;
        if self.timeout_ms == 0 || self.timeout_ms > 60_000 {
            return Err(DeviceError::Denied(
                "timeout_ms must be between 1 and 60000".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_9600_8n1() {
        let plan = termios_plan(&SerialOptions::default()).expect("plan");
        assert_eq!(plan.baud, 9600);
        assert_eq!(plan.char_bits, 8);
        assert!(!plan.parity_enable && !plan.parity_odd && !plan.two_stop_bits);
        assert!(!plan.hardware_flow && !plan.software_flow);
    }

    #[test]
    fn line_settings_map_to_flags() {
        let plan = termios_plan(&SerialOptions {
            baud: 115_200,
            data_bits: DataBits::Seven,
            parity: Parity::Odd,
            stop_bits: StopBits::Two,
            flow: FlowControl::Hardware,
        })
        .expect("plan");
        assert_eq!(plan.char_bits, 7);
        assert!(plan.parity_enable && plan.parity_odd && plan.two_stop_bits);
        assert!(plan.hardware_flow && !plan.software_flow);

        let even = termios_plan(&SerialOptions {
            parity: Parity::Even,
            flow: FlowControl::Software,
            ..SerialOptions::default()
        })
        .expect("plan");
        assert!(even.parity_enable && !even.parity_odd && even.software_flow);
    }

    #[test]
    fn nonstandard_bauds_are_refused() {
        for baud in [0, 1, 9601, 1_000_000] {
            let options = SerialOptions {
                baud,
                ..SerialOptions::default()
            };
            assert!(termios_plan(&options).is_err(), "accepted {baud}");
        }
    }

    #[test]
    fn open_options_check_the_timeout() {
        assert!(OpenOptions::default().validate().is_ok());
        let bad = OpenOptions {
            timeout_ms: 0,
            ..OpenOptions::default()
        };
        assert!(bad.validate().is_err());
    }
}
