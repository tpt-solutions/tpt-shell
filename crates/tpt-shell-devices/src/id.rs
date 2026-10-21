// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Device identifiers and scopes (spec §10.2, M7).
//!
//! Identifiers have a canonical text form, so they can be logged, compared and written to the
//! manifest:
//!
//! - `serial:COM3` (port names are compared without case)
//! - `hid:1234:5678` and `usb:1234:5678` (vendor and product, four hex digits each)
//! - `net:printer.local:9100` (host name and port)
//!
//! A [`DeviceScope`] matches identifiers with globs: `*` for any run of characters, `?` for
//! one. Port names and host names are matched without case. Scopes cannot match outside their
//! kind, so `serial:*` never matches a network device.

use std::fmt;
use std::str::FromStr;

/// One device, identified by what the bus or network reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceId {
    Serial { port: String },
    Hid { vendor: u16, product: u16 },
    Usb { vendor: u16, product: u16 },
    Net { host: String, port: u16 },
}

/// Why an identifier string was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceId::Serial { port } => write!(f, "serial:{port}"),
            DeviceId::Hid { vendor, product } => write!(f, "hid:{vendor:04x}:{product:04x}"),
            DeviceId::Usb { vendor, product } => write!(f, "usb:{vendor:04x}:{product:04x}"),
            DeviceId::Net { host, port } => write!(f, "net:{host}:{port}"),
        }
    }
}

impl FromStr for DeviceId {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        let (kind, rest) = s
            .split_once(':')
            .ok_or_else(|| ParseError(format!("`{s}` has no kind prefix")))?;
        match kind {
            "serial" => {
                check_port(rest)?;
                Ok(DeviceId::Serial {
                    port: rest.to_string(),
                })
            }
            "hid" | "usb" => {
                let (vendor, product) = parse_vid_pid(rest, s)?;
                Ok(if kind == "hid" {
                    DeviceId::Hid { vendor, product }
                } else {
                    DeviceId::Usb { vendor, product }
                })
            }
            "net" => {
                let (host, port) = rest
                    .rsplit_once(':')
                    .ok_or_else(|| ParseError(format!("`{s}` needs host:port")))?;
                check_host(host)?;
                let port: u16 = port
                    .parse()
                    .ok()
                    .filter(|p| *p != 0)
                    .ok_or_else(|| ParseError(format!("`{port}` is not a port number")))?;
                Ok(DeviceId::Net {
                    host: host.to_string(),
                    port,
                })
            }
            other => Err(ParseError(format!("unknown device kind `{other}`"))),
        }
    }
}

/// Port names: letters, digits, and `.`, `-`, `_`, such as `COM3` or `/dev/ttyUSB0`.
/// Slashes are allowed only at the start of a Unix device path.
fn check_port(port: &str) -> Result<(), ParseError> {
    let ok = !port.is_empty()
        && port.len() <= 64
        && port
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/'))
        && !port.contains("..")
        && !port.contains("//")
        && !port.ends_with('/')
        && (!port.contains('/') || port.starts_with("/dev/"));
    if ok {
        Ok(())
    } else {
        Err(ParseError(format!("`{port}` is not a serial port name")))
    }
}

fn check_host(host: &str) -> Result<(), ParseError> {
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(ParseError(format!("`{host}` is not a host name")))
    }
}

fn parse_vid_pid(rest: &str, whole: &str) -> Result<(u16, u16), ParseError> {
    let (vendor, product) = rest
        .split_once(':')
        .ok_or_else(|| ParseError(format!("`{whole}` needs vendor:product")))?;
    let hex = |s: &str| {
        if s.len() == 4 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            u16::from_str_radix(s, 16).ok()
        } else {
            None
        }
    };
    match (hex(vendor), hex(product)) {
        (Some(v), Some(p)) => Ok((v, p)),
        _ => Err(ParseError(format!(
            "`{whole}` needs four hex digits each for vendor and product"
        ))),
    }
}

/// Which devices a grant covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceScope {
    /// Serial ports whose names match the glob, for example `COM*`.
    Serial { port: String },
    /// HID devices. `None` means any value for that field.
    Hid {
        vendor: Option<u16>,
        product: Option<u16>,
    },
    /// USB devices. `None` means any value for that field.
    Usb {
        vendor: Option<u16>,
        product: Option<u16>,
    },
    /// Network devices whose host matches the glob and whose port matches, with `None` for any.
    Net { host: String, port: Option<u16> },
}

impl DeviceScope {
    /// Whether `id` falls inside this scope. A scope never matches another kind of device.
    pub fn matches(&self, id: &DeviceId) -> bool {
        match (self, id) {
            (DeviceScope::Serial { port }, DeviceId::Serial { port: p }) => {
                glob_match(&port.to_ascii_lowercase(), &p.to_ascii_lowercase())
            }
            (
                DeviceScope::Hid { vendor, product },
                DeviceId::Hid {
                    vendor: v,
                    product: p,
                },
            )
            | (
                DeviceScope::Usb { vendor, product },
                DeviceId::Usb {
                    vendor: v,
                    product: p,
                },
            ) => vendor.is_none_or(|x| x == *v) && product.is_none_or(|x| x == *p),
            (DeviceScope::Net { host, port }, DeviceId::Net { host: h, port: p }) => {
                glob_match(&host.to_ascii_lowercase(), &h.to_ascii_lowercase())
                    && port.is_none_or(|x| x == *p)
            }
            _ => false,
        }
    }
}

/// Glob match: `*` matches any run (including none), `?` matches one character.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Classic two-pointer match with backtracking to the last `*`.
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> DeviceId {
        s.parse().unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn identifiers_round_trip_through_their_text_form() {
        for s in [
            "serial:COM3",
            "serial:/dev/ttyUSB0",
            "hid:1a2b:00ff",
            "usb:0403:6001",
            "net:printer.local:9100",
        ] {
            assert_eq!(id(s).to_string(), s);
        }
    }

    #[test]
    fn malformed_identifiers_are_refused() {
        for bad in [
            "COM3",
            "serial:",
            "serial:COM3/../x",
            "serial:../etc",
            "serial:/etc/passwd",
            "serial:/dev//tty",
            "serial:/dev/tty/",
            "serial:/dev/serial/../../etc/shadow",
            "serial:COM3\\..\\x",
            "serial:COM3 extra",
            "hid:123:4567",
            "hid:zzzz:0001",
            "usb:1234",
            "net:printer.local",
            "net:printer.local:0",
            "net:printer.local:70000",
            "net:-bad-.local:9100",
            "net:a b:9100",
            "bogus:1",
        ] {
            assert!(bad.parse::<DeviceId>().is_err(), "accepted `{bad}`");
        }
    }

    #[test]
    fn glob_matches_stars_and_question_marks() {
        assert!(glob_match("com*", "com3"));
        assert!(glob_match("com?", "com3"));
        assert!(!glob_match("com?", "com31"));
        assert!(glob_match("*.local", "printer.local"));
        assert!(!glob_match("*.local", "printer.local.evil"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
    }

    #[test]
    fn serial_scopes_ignore_case_and_stay_in_kind() {
        let scope = DeviceScope::Serial {
            port: "COM*".into(),
        };
        assert!(scope.matches(&id("serial:com3")));
        assert!(!scope.matches(&id("net:com3.local:9100")));
        assert!(!scope.matches(&id("hid:0001:0002")));
    }

    #[test]
    fn hid_and_usb_scopes_match_vendor_and_product() {
        let any_product = DeviceScope::Hid {
            vendor: Some(0x1a2b),
            product: None,
        };
        assert!(any_product.matches(&id("hid:1a2b:0001")));
        assert!(!any_product.matches(&id("hid:1a2c:0001")));
        assert!(!any_product.matches(&id("usb:1a2b:0001")));

        let exact = DeviceScope::Usb {
            vendor: Some(0x0403),
            product: Some(0x6001),
        };
        assert!(exact.matches(&id("usb:0403:6001")));
        assert!(!exact.matches(&id("usb:0403:6002")));
    }

    #[test]
    fn network_scopes_match_host_globs_and_ports() {
        let scope = DeviceScope::Net {
            host: "*.local".into(),
            port: Some(9100),
        };
        assert!(scope.matches(&id("net:printer.local:9100")));
        assert!(!scope.matches(&id("net:printer.local:9101")));
        assert!(!scope.matches(&id("net:printer.example.com:9100")));
    }
}
