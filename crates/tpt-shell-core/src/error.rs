// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

use std::fmt;

/// Support tag for a public API item (spec §3).
///
/// Support: Full. A plain enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    /// Works on every supported platform.
    Full,
    /// Works, with the gap stated.
    Partial(&'static str),
    /// Returns [`Error::Unsupported`] on this platform.
    Unsupported,
}

/// Errors returned by the core and backend traits.
///
/// Support: Full. A plain enum; Unsupported is how every backend reports a missing feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The requested feature is not available on this platform or backend.
    Unsupported { feature: &'static str },
    /// The backend failed. The message is for developers, not end users.
    Backend(String),
    /// A window id was used that the backend does not know.
    UnknownWindow(u64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported { feature } => {
                write!(f, "feature not supported on this platform: {feature}")
            }
            Error::Backend(msg) => write!(f, "backend error: {msg}"),
            Error::UnknownWindow(id) => write!(f, "unknown window id {id}"),
        }
    }
}

impl std::error::Error for Error {}
/// Support: Full. An alias for the crate's error type.

pub type Result<T> = std::result::Result<T, Error>;
