// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Opens a shared library by trying a list of names, and looks up symbols in it.
//!
//! Errors name every library that was tried, and the symbol that was missing, so a user with
//! an old WebKitGTK sees which version the shell wanted (spec §6.3, `doctor`).

use std::fmt;

use libloading::Library;

/// Why a library could not be used.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
#[derive(Debug)]
pub enum LoadError {
    /// None of the candidate names could be opened.
    MissingLibrary {
        tried: Vec<String>,
        last_error: String,
    },
    /// The library opened but does not export this symbol.
    MissingSymbol { library: String, symbol: String },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::MissingLibrary { tried, last_error } => write!(
                f,
                "could not load any of {}: {last_error}. Install the runtime package for this library",
                tried.join(", ")
            ),
            LoadError::MissingSymbol { library, symbol } => write!(
                f,
                "{library} does not provide `{symbol}`; the installed version is too old or too new"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// A loaded library. Symbols borrowed from it must not outlive it.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
pub struct SharedLibrary {
    name: String,
    lib: Library,
}

impl SharedLibrary {
    /// Opens the first of `names` that loads.
    ///
    /// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
    /// checked.
    pub fn open(names: &[&str]) -> Result<Self, LoadError> {
        let mut last_error = String::from("no candidate names were given");
        for name in names {
            // SAFETY: opening a library runs its initialisers. The names are the fixed
            // library names of the GTK and WebKit runtime packages, chosen by the caller.
            match unsafe { Library::new(name) } {
                Ok(lib) => {
                    return Ok(SharedLibrary {
                        name: (*name).to_string(),
                        lib,
                    })
                }
                Err(e) => last_error = e.to_string(),
            }
        }
        Err(LoadError::MissingLibrary {
            tried: names.iter().map(|s| (*s).to_string()).collect(),
            last_error,
        })
    }

    /// The name that opened successfully.
    ///
    /// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
    /// checked.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Looks up a function or variable of type `T`. `T` must match the C declaration.
    ///
    /// # Safety
    /// The caller guarantees that `T` is the exact type of the exported symbol, including the
    /// calling convention. A wrong type is undefined behaviour when the symbol is called.
    ///
    /// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
    /// checked.
    pub unsafe fn symbol<T: Copy>(&self, symbol: &str) -> Result<T, LoadError> {
        // SAFETY: the caller upholds the type contract documented above.
        let sym: libloading::Symbol<'_, T> =
            self.lib
                .get(symbol.as_bytes())
                .map_err(|_| LoadError::MissingSymbol {
                    library: self.name.clone(),
                    symbol: symbol.to_string(),
                })?;
        Ok(*sym)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_names_every_candidate() {
        let err = match SharedLibrary::open(&[
            "libtpt-definitely-missing-1.so",
            "libtpt-definitely-missing-2.so",
        ]) {
            Err(e) => e,
            Ok(_) => panic!("a library that does not exist must not load"),
        };
        match &err {
            LoadError::MissingLibrary { tried, .. } => {
                assert_eq!(tried.len(), 2);
            }
            other => panic!("expected MissingLibrary, got {other:?}"),
        }
        let text = err.to_string();
        assert!(text.contains("libtpt-definitely-missing-1.so"));
        assert!(text.contains("libtpt-definitely-missing-2.so"));
    }

    #[test]
    fn no_candidates_is_an_error_not_a_panic() {
        assert!(SharedLibrary::open(&[]).is_err());
    }
}
