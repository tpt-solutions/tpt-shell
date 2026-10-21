// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The system light or dark setting, read from the desktop portal (`org.freedesktop.portal`
//! Settings, namespace `org.freedesktop.appearance`, key `color-scheme`).
//!
//! The portal value is `0` for no preference, `1` for dark and `2` for light. No preference is
//! reported as light, the default of [`ColorScheme`].

use tpt_shell_core::{ColorScheme, Error, Result};

use super::bus::{session, Connection, Stream};
use super::wire::Value;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS_IFACE: &str = "org.freedesktop.portal.Settings";

/// Maps the portal's `color-scheme` value to a [`ColorScheme`].
pub fn scheme_from_portal(value: u32) -> ColorScheme {
    match value {
        1 => ColorScheme::Dark,
        _ => ColorScheme::Light,
    }
}

/// Reads the colour scheme. Needs a running desktop portal; otherwise it fails with an error.
pub fn color_scheme() -> Result<ColorScheme> {
    let mut conn: Connection<Stream> = session()?;
    read_color_scheme(&mut conn)
}

fn read_color_scheme(conn: &mut Connection<Stream>) -> Result<ColorScheme> {
    let reply = conn.call(
        PORTAL,
        PORTAL_PATH,
        SETTINGS_IFACE,
        "Read",
        vec![
            Value::str("org.freedesktop.appearance"),
            Value::str("color-scheme"),
        ],
    )?;
    // Older portals wrap the value in one variant more than newer ones; unwrap either way.
    let value = reply
        .first()
        .map(Value::unwrap_variant)
        .and_then(|v| match v {
            Value::Uint32(n) => Some(*n),
            _ => None,
        })
        .ok_or_else(|| Error::Backend("the portal gave no colour scheme".into()))?;
    Ok(scheme_from_portal(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_values_map_to_schemes() {
        assert_eq!(scheme_from_portal(1), ColorScheme::Dark);
        assert_eq!(scheme_from_portal(2), ColorScheme::Light);
        assert_eq!(
            scheme_from_portal(0),
            ColorScheme::Light,
            "no preference is light"
        );
        assert_eq!(
            scheme_from_portal(99),
            ColorScheme::Light,
            "unknown values are light"
        );
    }

    #[test]
    fn nested_variants_are_unwrapped() {
        let reply = [Value::variant(Value::variant(Value::Uint32(1)))];
        let value = reply
            .first()
            .map(Value::unwrap_variant)
            .and_then(|v| match v {
                Value::Uint32(n) => Some(*n),
                _ => None,
            });
        assert_eq!(value, Some(1));
    }
}
