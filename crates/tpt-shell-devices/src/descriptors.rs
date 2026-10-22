// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Parsers for what Linux (sysfs, hidraw uevent, usbfs) and USB itself report about a device
//! (spec §10.2, M7). They are pure functions over text and bytes, so they are tested on every
//! host, and the Linux backends feed them what they read from the system.

use crate::id::DeviceId;

/// What a hidraw `uevent` file says about a device.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HidInfo {
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub name: String,
}

impl HidInfo {
    /// The device identifier for this device.
    ///
    /// Support: Full.
    pub fn id(&self) -> DeviceId {
        DeviceId::Hid {
            vendor: self.vendor,
            product: self.product,
        }
    }
}

/// Parses the `HID_ID=BBBB:VVVVVVVV:PPPPPPPP` and `HID_NAME=` lines of a hidraw `uevent`.
/// Returns `None` when there is no well-formed `HID_ID`, or when a vendor or product does not fit
/// 16 bits.
///
/// Support: Full.
pub fn parse_hid_uevent(text: &str) -> Option<HidInfo> {
    let mut ids = None;
    let mut name = String::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("HID_ID=") {
            let mut parts = value.trim().split(':');
            let bus = u32::from_str_radix(parts.next()?, 16).ok()?;
            let vendor = u32::from_str_radix(parts.next()?, 16).ok()?;
            let product = u32::from_str_radix(parts.next()?, 16).ok()?;
            if parts.next().is_some() {
                return None;
            }
            ids = Some((
                u16::try_from(bus).ok()?,
                u16::try_from(vendor).ok()?,
                u16::try_from(product).ok()?,
            ));
        } else if let Some(value) = line.strip_prefix("HID_NAME=") {
            name = value.trim().to_string();
        }
    }
    let (bus, vendor, product) = ids?;
    Some(HidInfo {
        bus,
        vendor,
        product,
        name,
    })
}

/// Parses a sysfs hex attribute such as `idVendor` (`046d\n`).
///
/// Support: Full.
pub fn parse_sysfs_hex_u16(text: &str) -> Option<u16> {
    let t = text.trim();
    if t.is_empty() || t.len() > 4 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(t, 16).ok()
}

/// A USB interface with its bulk endpoints (alternate setting 0 only).
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub number: u8,
    pub bulk_in: Option<u8>,
    pub bulk_out: Option<u8>,
}

/// The parts of the USB descriptors the raw transport uses.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbInfo {
    pub vendor: u16,
    pub product: u16,
    pub interfaces: Vec<InterfaceInfo>,
}

impl UsbInfo {
    /// The device identifier for this device.
    ///
    /// Support: Full.
    pub fn id(&self) -> DeviceId {
        DeviceId::Usb {
            vendor: self.vendor,
            product: self.product,
        }
    }
}

/// Parses the descriptors usbfs returns when a device node is read: the 18-byte device
/// descriptor, then the configuration descriptor with its interface and endpoint descriptors.
/// Returns `None` for a truncated or malformed block.
///
/// Support: Full.
pub fn parse_usb_descriptors(bytes: &[u8]) -> Option<UsbInfo> {
    if bytes.len() < 18 || bytes[0] != 18 || bytes[1] != 1 {
        return None;
    }
    let vendor = u16::from_le_bytes([bytes[8], bytes[9]]);
    let product = u16::from_le_bytes([bytes[10], bytes[11]]);
    let mut interfaces: Vec<InterfaceInfo> = Vec::new();
    let mut current: Option<usize> = None;
    let mut rest = &bytes[18..];
    while !rest.is_empty() {
        let len = *rest.first()? as usize;
        if len < 2 || len > rest.len() {
            return None;
        }
        let (d, tail) = rest.split_at(len);
        match d[1] {
            4 if len >= 9 => {
                if d[3] == 0 {
                    interfaces.push(InterfaceInfo {
                        number: d[2],
                        bulk_in: None,
                        bulk_out: None,
                    });
                    current = Some(interfaces.len() - 1);
                } else {
                    current = None;
                }
            }
            5 if len >= 7 => {
                if let Some(i) = current {
                    let address = d[2];
                    if d[3] & 3 == 2 {
                        let slot = if address & 0x80 != 0 {
                            &mut interfaces[i].bulk_in
                        } else {
                            &mut interfaces[i].bulk_out
                        };
                        slot.get_or_insert(address);
                    }
                }
            }
            _ => {}
        }
        rest = tail;
    }
    Some(UsbInfo {
        vendor,
        product,
        interfaces,
    })
}

/// The usbfs node for a bus and device number.
///
/// Support: Full.
pub fn usbfs_path(bus: u16, device: u16) -> String {
    format!("/dev/bus/usb/{bus:03}/{device:03}")
}

/// Whether `name` (a file name under `/dev`) is a serial port worth listing: USB serial, ACM,
/// on-board UARTs and the macOS callout nodes.
///
/// Support: Full.
pub fn is_serial_node(name: &str) -> bool {
    let numbered = |prefix: &str| {
        name.strip_prefix(prefix)
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    numbered("ttyUSB")
        || numbered("ttyACM")
        || numbered("ttyS")
        || numbered("ttyAMA")
        || name.strip_prefix("cu.").is_some_and(|n| {
            !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        })
}

/// The Linux ioctl request number for a direction (`1` write, `2` read), type, number and
/// argument size, in the encoding used by x86, ARM and RISC-V (2 direction bits, 14 size bits).
///
/// Support: Full.
pub const fn ioc(dir: u32, ty: u8, nr: u8, size: usize) -> u32 {
    (dir << 30) | ((size as u32) << 16) | ((ty as u32) << 8) | nr as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidraw_uevent_gives_the_vendor_product_and_name() {
        let text = "DRIVER=hid-generic\nHID_ID=0003:0000046D:0000C31C\nHID_NAME=Logitech USB Keyboard\nHID_PHYS=usb-0000:00:14.0-1/input0\n";
        let info = parse_hid_uevent(text).expect("parsed");
        assert_eq!(info.bus, 3);
        assert_eq!(info.vendor, 0x046d);
        assert_eq!(info.product, 0xc31c);
        assert_eq!(info.name, "Logitech USB Keyboard");
        assert_eq!(info.id().to_string(), "hid:046d:c31c");
    }

    #[test]
    fn a_bad_uevent_is_refused() {
        assert!(parse_hid_uevent("").is_none());
        assert!(parse_hid_uevent("HID_ID=0003:046D").is_none());
        assert!(parse_hid_uevent("HID_ID=0003:zzzz:0001").is_none());
        // A vendor wider than 16 bits does not fit a DeviceId.
        assert!(parse_hid_uevent("HID_ID=0003:00010000:00000001").is_none());
        assert!(parse_hid_uevent("HID_ID=0003:0001:0002:0003").is_none());
    }

    #[test]
    fn sysfs_hex_is_four_digits_at_most() {
        assert_eq!(parse_sysfs_hex_u16("046d\n"), Some(0x046d));
        assert_eq!(parse_sysfs_hex_u16("6001"), Some(0x6001));
        assert_eq!(parse_sysfs_hex_u16(""), None);
        assert_eq!(parse_sysfs_hex_u16("12345"), None);
        assert_eq!(parse_sysfs_hex_u16("0x12"), None);
    }

    fn sample() -> Vec<u8> {
        let mut b = vec![
            18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x03, 0x04, 0x01, 0x60, 0, 1, 1, 2, 3, 1,
        ];
        // Configuration descriptor.
        b.extend([9, 2, 32, 0, 1, 1, 0, 0x80, 50]);
        // Interface 0, alt 0, two endpoints.
        b.extend([9, 4, 0, 0, 2, 0xff, 0, 0, 0]);
        b.extend([7, 5, 0x81, 2, 64, 0, 0]);
        b.extend([7, 5, 0x02, 2, 64, 0, 0]);
        // Interface 0, alt 1: ignored.
        b.extend([9, 4, 0, 1, 1, 0xff, 0, 0, 0]);
        b.extend([7, 5, 0x83, 2, 64, 0, 0]);
        b
    }

    #[test]
    fn usb_descriptors_give_ids_and_bulk_endpoints() {
        let info = parse_usb_descriptors(&sample()).expect("parsed");
        assert_eq!(info.vendor, 0x0403);
        assert_eq!(info.product, 0x6001);
        assert_eq!(info.id().to_string(), "usb:0403:6001");
        assert_eq!(
            info.interfaces,
            vec![InterfaceInfo {
                number: 0,
                bulk_in: Some(0x81),
                bulk_out: Some(0x02)
            }]
        );
    }

    #[test]
    fn truncated_or_wrong_descriptors_are_refused() {
        assert!(parse_usb_descriptors(&[]).is_none());
        assert!(parse_usb_descriptors(&[0; 18]).is_none());
        let mut cut = sample();
        cut.truncate(cut.len() - 3);
        assert!(parse_usb_descriptors(&cut).is_none());
        let mut zero_len = sample();
        zero_len.extend([0, 0]);
        assert!(parse_usb_descriptors(&zero_len).is_none());
    }

    #[test]
    fn usbfs_paths_are_zero_padded() {
        assert_eq!(usbfs_path(1, 7), "/dev/bus/usb/001/007");
    }

    #[test]
    fn serial_nodes_are_recognised() {
        for yes in [
            "ttyUSB0",
            "ttyACM12",
            "ttyS0",
            "ttyAMA0",
            "cu.usbserial-1410",
        ] {
            assert!(is_serial_node(yes), "{yes}");
        }
        for no in [
            "tty",
            "ttyUSB",
            "tty0",
            "null",
            "cu.",
            "ttyUSB0x",
            "tty.usbserial",
        ] {
            assert!(!is_serial_node(no), "{no}");
        }
    }

    #[test]
    fn ioctl_numbers_match_the_kernel_headers() {
        // USBDEVFS_CLAIMINTERFACE = _IOR('U', 15, unsigned int) = 0x8004550f.
        assert_eq!(ioc(2, b'U', 15, 4), 0x8004_550f);
        // USBDEVFS_RESET = _IO('U', 20) = 0x5514.
        assert_eq!(ioc(0, b'U', 20, 0), 0x5514);
        // USBDEVFS_CONTROL = _IOWR('U', 0, struct usbdevfs_ctrltransfer) is 0xc0185500 on 64-bit.
        assert_eq!(ioc(3, b'U', 0, 24), 0xc018_5500);
    }
}
