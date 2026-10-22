// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Freedesktop desktop notifications (`org.freedesktop.Notifications.Notify`).
//!
//! Partial: notifications are shown, but action buttons and the close and activation signals
//! are not read, so the app cannot react to a click on one.

use tpt_shell_core::{Error, Result};

use super::bus::{session, Connection, Stream};
use super::wire::Value;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";

/// The longest summary or body accepted, in bytes. A longer text is refused, not cut.
///
/// Support: Full.
pub const MAX_TEXT: usize = 1024;

/// One notification to show.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// Shown for this many milliseconds. `None` lets the server choose.
    pub timeout_ms: Option<i32>,
}

/// A connection used to show notifications.
///
/// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
/// Unsupported. Not run on macOS/Linux here; compile-checked.
pub struct Notifier {
    conn: Connection<Stream>,
}

impl Notifier {
    /// Connects to the session bus. Fails with `Unsupported` without the `dbus` feature.
    ///
    /// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
    /// Unsupported. Not run on macOS/Linux here; compile-checked.
    pub fn connect() -> Result<Notifier> {
        Ok(Notifier { conn: session()? })
    }

    /// Shows the notification and returns the id the server gave it.
    ///
    /// Support: Partial. Behind the `dbus` feature, which is off by default and then returns
    /// Unsupported. Not run on macOS/Linux here; compile-checked.
    pub fn show(&mut self, note: &Notification) -> Result<u32> {
        check_text(&note.summary)?;
        check_text(&note.body)?;
        check_text(&note.app_name)?;
        let body = vec![
            Value::str(&note.app_name),
            Value::Uint32(0),
            Value::str(""),
            Value::str(&note.summary),
            Value::str(&escape_markup(&note.body)),
            Value::Array("s".to_string(), vec![]),
            Value::dict_sv(vec![]),
            Value::Int32(note.timeout_ms.unwrap_or(-1)),
        ];
        let reply = self.conn.call(SERVICE, PATH, INTERFACE, "Notify", body)?;
        reply
            .first()
            .and_then(Value::as_u32)
            .ok_or_else(|| Error::Backend("the notification server gave no id".into()))
    }
}

fn check_text(text: &str) -> Result<()> {
    if text.len() > MAX_TEXT {
        return Err(Error::Backend(format!(
            "notification text is longer than {MAX_TEXT} bytes"
        )));
    }
    if text.contains('\0') {
        return Err(Error::Backend("notification text holds a NUL byte".into()));
    }
    Ok(())
}

/// Escapes the three characters the server reads as markup, so page text cannot inject
/// formatting or links into the notification.
///
/// Support: Full.
pub fn escape_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_are_escaped() {
        assert_eq!(
            escape_markup("<a href=x>&</a>"),
            "&lt;a href=x&gt;&amp;&lt;/a&gt;"
        );
    }

    #[test]
    fn oversized_or_nul_text_is_refused() {
        assert!(check_text(&"x".repeat(MAX_TEXT + 1)).is_err());
        assert!(check_text("a\0b").is_err());
        assert!(check_text("ok").is_ok());
    }

    #[test]
    fn without_the_feature_showing_a_notification_is_unsupported() {
        #[cfg(not(all(feature = "dbus", unix)))]
        {
            assert!(matches!(
                Notifier::connect(),
                Err(Error::Unsupported { .. })
            ));
        }
    }
}
