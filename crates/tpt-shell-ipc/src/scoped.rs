// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Scoped argument types for commands (spec §8): a path or a device identifier that is checked
//! when it is read from the arguments, so a handler never holds a value that failed the check.
//!
//! [`ScopedPath`] must be an absolute path that passes [`tpt_shell_capabilities::normalize_path`]:
//! no relative paths, `..`, UNC or device paths, alternate data streams or reserved names. It
//! is a lexical check. A handler that opens the file must still use
//! [`tpt_shell_capabilities::real_path`] or the engine's `check_path_on_disk`, so that a link
//! cannot lead out of a granted root.
//!
//! [`DeviceId`] is a short identifier such as `serial:COM3` or `usb:0416:5011`: ASCII letters,
//! digits and `_ . : -`, at most 128 bytes.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::reflect::ToSchema;
use crate::schema::Schema;

/// Longest path accepted in a [`ScopedPath`] (bytes). Windows paths are limited to 32767 UTF-16
/// units, but a much shorter limit is enough for a command argument.
pub const MAX_PATH_LEN: usize = 1024;
/// Longest device identifier accepted in a [`DeviceId`] (bytes).
pub const MAX_DEVICE_ID_LEN: usize = 128;
/// Characters allowed in a device identifier, besides ASCII letters and digits.
const DEVICE_ID_EXTRA: &str = "_.:-";

/// Why a scoped value was refused. The message names the problem and is safe to show a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeError(pub String);

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScopeError {}

/// An absolute, lexically safe path from the page. The text is kept as given, so the handler can
/// open it. Build one with [`ScopedPath::new`]; the serde and argument paths go through it too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ScopedPath(String);

impl ScopedPath {
    /// Checks `raw` and wraps it.
    pub fn new(raw: &str) -> Result<Self, ScopeError> {
        if raw.len() > MAX_PATH_LEN {
            return Err(ScopeError(format!(
                "path is {} bytes, limit is {MAX_PATH_LEN}",
                raw.len()
            )));
        }
        tpt_shell_capabilities::normalize_path(raw)
            .map_err(|why| ScopeError(format!("`{raw}` is not a safe path: {why}")))?;
        Ok(ScopedPath(raw.to_string()))
    }

    /// The path as given.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The lexically normalized path, for comparing with a scope. See
    /// [`tpt_shell_capabilities::normalize_path`].
    pub fn normalized(&self) -> String {
        // Checked in `new`, so this does not fail.
        tpt_shell_capabilities::normalize_path(&self.0).unwrap_or_default()
    }
}

impl TryFrom<String> for ScopedPath {
    type Error = ScopeError;

    fn try_from(raw: String) -> Result<Self, ScopeError> {
        ScopedPath::new(&raw)
    }
}

impl From<ScopedPath> for String {
    fn from(path: ScopedPath) -> String {
        path.0
    }
}

impl ToSchema for ScopedPath {
    fn schema() -> Schema {
        Schema::Str {
            max_len: MAX_PATH_LEN,
            chars: None,
        }
    }
}

/// A device identifier from the page, such as `serial:COM3`. See the module documentation for
/// the allowed characters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceId(String);

impl DeviceId {
    /// Checks `raw` and wraps it.
    pub fn new(raw: &str) -> Result<Self, ScopeError> {
        if raw.is_empty() {
            return Err(ScopeError("device id is empty".into()));
        }
        if raw.len() > MAX_DEVICE_ID_LEN {
            return Err(ScopeError(format!(
                "device id is {} bytes, limit is {MAX_DEVICE_ID_LEN}",
                raw.len()
            )));
        }
        let allowed = raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || DEVICE_ID_EXTRA.contains(c));
        if !allowed {
            return Err(ScopeError(format!(
                "`{raw}` has a character that is not allowed in a device id"
            )));
        }
        Ok(DeviceId(raw.to_string()))
    }

    /// The identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DeviceId {
    type Error = ScopeError;

    fn try_from(raw: String) -> Result<Self, ScopeError> {
        DeviceId::new(&raw)
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> String {
        id.0
    }
}

impl ToSchema for DeviceId {
    fn schema() -> Schema {
        Schema::Str {
            max_len: MAX_DEVICE_ID_LEN,
            chars: Some("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.:-"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_safe_absolute_path_is_accepted_and_kept_as_given() {
        let p = ScopedPath::new("C:\\Data\\a.txt").unwrap();
        assert_eq!(p.as_str(), "C:\\Data\\a.txt");
        assert_eq!(p.normalized(), "c:/data/a.txt");
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for bad in [
            "",
            "relative/a.txt",
            "C:/Data/../etc",
            "\\\\server\\share\\a",
            "\\\\?\\C:\\a",
            "C:/Data/a.txt:stream",
            "C:/Data/CON.txt",
            "C:/Data/a.",
            "C:/Data/a\0b",
        ] {
            assert!(ScopedPath::new(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn over_long_paths_are_refused() {
        let long = format!("C:/{}", "a".repeat(MAX_PATH_LEN));
        assert!(ScopedPath::new(&long).is_err());
    }

    #[test]
    fn a_scoped_path_is_checked_when_it_is_read_from_json() {
        let ok: ScopedPath = serde_json::from_value(json!("C:/Data/a.txt")).unwrap();
        assert_eq!(ok.as_str(), "C:/Data/a.txt");
        assert!(serde_json::from_value::<ScopedPath>(json!("../secret")).is_err());
        assert!(serde_json::from_value::<ScopedPath>(json!(7)).is_err());
    }

    #[test]
    fn device_ids_allow_only_the_identifier_characters() {
        assert!(DeviceId::new("serial:COM3").is_ok());
        assert!(DeviceId::new("usb:0416:5011").is_ok());
        for bad in ["", "a b", "a/b", "a\\b", "COM3;rm", "é", &"x".repeat(129)] {
            assert!(DeviceId::new(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn scoped_types_describe_themselves_to_the_contract() {
        assert_eq!(
            ScopedPath::schema(),
            Schema::Str {
                max_len: MAX_PATH_LEN,
                chars: None
            }
        );
        assert!(matches!(
            DeviceId::schema(),
            Schema::Str {
                max_len: MAX_DEVICE_ID_LEN,
                chars: Some(_)
            }
        ));
    }
}
