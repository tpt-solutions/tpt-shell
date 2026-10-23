// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The device scope a command opens a device with, taken from the window's grant. The capability
//! engine checks a page's request against the grant's entries. The device hub checks the device
//! against the scope the command passes. Building that scope from the same entries keeps the two
//! checks in agreement: a device the grant does not cover is refused by both.

use tpt_shell_capabilities::{Grant, Scope};
use tpt_shell_devices::id::{DeviceId, DeviceScope};

/// The scope a grant entry names, such as `usb:0416:*`, `hid:0416:5011`, `serial:COM*` or
/// `net:*.local:9100`. In vendor, product and port fields `*` means any value. The entry's kind
/// prefix picks the variant. Host and port globs are passed to the hub as written.
///
/// Support: Partial. Serial and net names are matched as globs by the hub, not checked against a
/// real port or host. Hexadecimal vendor and product values are read as written and not checked
/// against any device database.
pub fn scope_for_entry(entry: &str) -> Result<DeviceScope, String> {
    let (kind, rest) = entry
        .split_once(':')
        .ok_or_else(|| format!("`{entry}` has no kind prefix"))?;
    match kind {
        "serial" => {
            if rest.is_empty() {
                return Err(format!("`{entry}` names no port"));
            }
            Ok(DeviceScope::Serial { port: rest.into() })
        }
        "hid" | "usb" => {
            let (vendor, product) = rest
                .split_once(':')
                .ok_or_else(|| format!("`{entry}` needs vendor:product"))?;
            let vendor = hex_or_any(vendor, entry)?;
            let product = hex_or_any(product, entry)?;
            Ok(if kind == "hid" {
                DeviceScope::Hid { vendor, product }
            } else {
                DeviceScope::Usb { vendor, product }
            })
        }
        "net" => {
            let (host, port) = rest
                .rsplit_once(':')
                .ok_or_else(|| format!("`{entry}` needs host:port"))?;
            if host.is_empty() {
                return Err(format!("`{entry}` names no host"));
            }
            let port = if port == "*" {
                None
            } else {
                Some(
                    port.parse::<u16>()
                        .map_err(|_| format!("`{port}` is not a port number"))?,
                )
            };
            Ok(DeviceScope::Net {
                host: host.into(),
                port,
            })
        }
        other => Err(format!("unknown device kind `{other}`")),
    }
}

/// `*` is `None` (any value); anything else must be a 16-bit hexadecimal number.
fn hex_or_any(field: &str, entry: &str) -> Result<Option<u16>, String> {
    if field == "*" {
        return Ok(None);
    }
    u16::from_str_radix(field, 16)
        .map(Some)
        .map_err(|_| format!("`{field}` in `{entry}` is not a hexadecimal 16-bit value or `*`"))
}

/// The scope of the first device entry in `grant` that covers `id`, or `None` when the grant
/// covers no entry for it. A grant with no device entries (any other capability) gives `None`.
///
/// Support: Full. Entries that do not parse are skipped, so an unusable entry can never widen a
/// scope; the capability engine refuses such entries when the grant is added.
pub fn scope_from_grant(grant: &Grant, id: &DeviceId) -> Option<DeviceScope> {
    let Scope::Devices(entries) = &grant.scope else {
        return None;
    };
    entries
        .iter()
        .filter_map(|entry| scope_for_entry(entry).ok())
        .find(|scope| scope.matches(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_shell_capabilities::{Decision, Engine, Request};

    const ORIGIN: &str = "https://tpt-app.localhost";

    fn grant(capability: &str, entries: &[&str]) -> Grant {
        Grant {
            capability: capability.into(),
            window: "main".into(),
            origin: ORIGIN.into(),
            scope: Scope::Devices(entries.iter().map(|e| e.to_string()).collect()),
            consent: false,
        }
    }

    fn id(s: &str) -> DeviceId {
        s.parse().expect("device id")
    }

    #[test]
    fn entries_parse_into_the_scope_their_kind_names() {
        assert_eq!(
            scope_for_entry("usb:0416:*"),
            Ok(DeviceScope::Usb {
                vendor: Some(0x0416),
                product: None
            })
        );
        assert_eq!(
            scope_for_entry("hid:0416:5011"),
            Ok(DeviceScope::Hid {
                vendor: Some(0x0416),
                product: Some(0x5011)
            })
        );
        assert_eq!(
            scope_for_entry("serial:COM*"),
            Ok(DeviceScope::Serial {
                port: "COM*".into()
            })
        );
        assert_eq!(
            scope_for_entry("net:*.local:9100"),
            Ok(DeviceScope::Net {
                host: "*.local".into(),
                port: Some(9100)
            })
        );
        assert_eq!(
            scope_for_entry("net:printer.local:*"),
            Ok(DeviceScope::Net {
                host: "printer.local".into(),
                port: None
            })
        );
    }

    #[test]
    fn malformed_entries_are_refused_not_widened() {
        for bad in [
            "usb:0416",
            "usb:zz:1",
            "hid:12345:1",
            "serial:",
            "net:9100",
            "net::9100",
            "net:host:http",
            "bluetooth:AA",
            "nokind",
        ] {
            assert!(scope_for_entry(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn the_derived_scope_covers_only_what_the_grant_names() {
        let g = grant("device.print", &["usb:0416:*", "serial:COM*"]);
        assert!(scope_from_grant(&g, &id("usb:0416:5011")).is_some());
        assert!(scope_from_grant(&g, &id("usb:0417:0001")).is_none());
        assert!(scope_from_grant(&g, &id("serial:COM3")).is_some());
        assert!(scope_from_grant(&g, &id("serial:LPT1")).is_none());
        assert!(scope_from_grant(&g, &id("hid:0416:5011")).is_none());
    }

    #[test]
    fn the_engine_and_the_derived_scope_agree_on_a_device_outside_the_grant() {
        // The page's request goes through the engine, and the command opens the device with the
        // scope derived from the same grant. A device outside the grant fails both checks.
        let g = grant("device.print", &["usb:0416:*"]);
        let mut engine = Engine::new();
        engine.add_grant(g.clone()).expect("grant");

        let inside = id("usb:0416:5011");
        assert_eq!(
            engine.check(
                "main",
                ORIGIN,
                "device.print",
                Request::Device(&inside.to_string())
            ),
            Decision::Allow
        );
        assert!(scope_from_grant(&g, &inside)
            .expect("scope")
            .matches(&inside));

        let outside = id("usb:0417:0001");
        assert!(matches!(
            engine.check(
                "main",
                ORIGIN,
                "device.print",
                Request::Device(&outside.to_string())
            ),
            Decision::Deny(_)
        ));
        assert!(scope_from_grant(&g, &outside).is_none());
    }
}
