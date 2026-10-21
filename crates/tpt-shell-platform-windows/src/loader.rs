// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! OfficialLoader strategy (spec §6.1, decision O1): loads `WebView2Loader.dll` and
//! resolves its exports. The loader ships with the app; see THIRD_PARTY_NOTICES.

use std::ffi::c_void;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

/// Exports this strategy needs. Checked on load so a bad DLL fails early.
pub const ENTRY_POINTS: [&str; 3] = [
    "CreateCoreWebView2EnvironmentWithOptions",
    "CreateCoreWebView2Environment",
    "GetAvailableCoreWebView2BrowserVersionString",
];

/// `HRESULT CreateCoreWebView2EnvironmentWithOptions(browserFolder, userDataFolder, options, handler)`.
pub type CreateEnvironmentFn =
    unsafe extern "system" fn(*const u16, *const u16, *mut c_void, *mut c_void) -> i32;

type GetVersionFn =
    unsafe extern "system" fn(browser_folder: *const u16, version: *mut *mut u16) -> i32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The DLL could not be loaded. Includes the path tried.
    NotFound(String),
    /// The DLL loaded but lacks an export. Names the export.
    MissingExport(&'static str),
    /// The version query returned an error code.
    VersionQueryFailed(i32),
    /// The version query returned an empty string.
    EmptyVersion,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound(p) => write!(f, "could not load WebView2Loader.dll from {p}"),
            LoadError::MissingExport(name) => {
                write!(f, "WebView2Loader.dll has no export `{name}`")
            }
            LoadError::VersionQueryFailed(hr) => {
                write!(f, "runtime version query failed: HRESULT 0x{hr:08X}")
            }
            LoadError::EmptyVersion => write!(f, "runtime version query returned an empty string"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A loaded `WebView2Loader.dll`. Unloaded on drop.
pub struct OfficialLoader {
    module: HMODULE,
}

impl OfficialLoader {
    /// Loads the DLL at `path` and checks that every entry point in [`ENTRY_POINTS`] exists.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is a valid, NUL-terminated UTF-16 string that outlives the call.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return Err(LoadError::NotFound(path.display().to_string()));
        }
        let loader = OfficialLoader { module };
        for name in ENTRY_POINTS {
            if loader.raw_export(name).is_none() {
                return Err(LoadError::MissingExport(name));
            }
        }
        Ok(loader)
    }

    /// Returns the address of an export, or `None` if it does not exist.
    fn raw_export(&self, name: &str) -> Option<unsafe extern "system" fn() -> isize> {
        let cname: Vec<u8> = name.bytes().chain(std::iter::once(0)).collect();
        // SAFETY: `module` is a live handle from LoadLibraryW; `cname` is NUL-terminated.
        unsafe { GetProcAddress(self.module, cname.as_ptr()) }
    }

    /// The environment entry point, typed. Valid while this loader is alive.
    pub fn create_environment_fn(&self) -> Result<CreateEnvironmentFn, LoadError> {
        let proc = self
            .raw_export("CreateCoreWebView2EnvironmentWithOptions")
            .ok_or(LoadError::MissingExport(
                "CreateCoreWebView2EnvironmentWithOptions",
            ))?;
        // SAFETY: the export has the documented signature (see `CreateEnvironmentFn`), and both
        // are plain function pointers of the same size. The DLL stays loaded while `self` lives.
        Ok(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, CreateEnvironmentFn>(proc)
        })
    }

    /// Version of the installed WebView2 Evergreen runtime, for example "155.0.4283.62".
    pub fn runtime_version(&self) -> Result<String, LoadError> {
        let proc = self
            .raw_export("GetAvailableCoreWebView2BrowserVersionString")
            .ok_or(LoadError::MissingExport(
                "GetAvailableCoreWebView2BrowserVersionString",
            ))?;
        // SAFETY: the export has the documented signature `HRESULT f(LPCWSTR, LPWSTR*)`.
        // Passing a null folder selects the installed Evergreen runtime.
        let get_version: GetVersionFn = unsafe { std::mem::transmute(proc) };
        let mut out: *mut u16 = std::ptr::null_mut();
        // SAFETY: `out` is a valid out-pointer. On success the callee allocates the string with
        // CoTaskMemFree semantics, which we honour below.
        let hr = unsafe { get_version(std::ptr::null(), &mut out) };
        if hr < 0 {
            return Err(LoadError::VersionQueryFailed(hr));
        }
        if out.is_null() {
            return Err(LoadError::EmptyVersion);
        }
        let mut len = 0usize;
        // SAFETY: the callee returns a NUL-terminated UTF-16 string, so reading until NUL stays in bounds.
        unsafe {
            while *out.add(len) != 0 {
                len += 1;
            }
        }
        // SAFETY: `out` is a valid slice of `len` u16 values, read-only for the copy below.
        let version = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(out, len) });
        // SAFETY: `out` was allocated by the callee with CoTaskMemFree semantics and is freed once.
        unsafe { CoTaskMemFree(out as *const c_void) };
        if version.is_empty() {
            return Err(LoadError::EmptyVersion);
        }
        Ok(version)
    }
}

/// How the shell finds and talks to the WebView2 runtime (spec §6.1). The shell only uses
/// [`OfficialLoader`] (decision O1). The trait lets other strategies, and test doubles, plug in.
pub trait RuntimeLoader {
    /// The environment entry point, typed.
    fn create_environment_fn(&self) -> Result<CreateEnvironmentFn, LoadError>;
    /// Version of the installed runtime, for example "155.0.4283.62".
    fn runtime_version(&self) -> Result<String, LoadError>;
}

impl RuntimeLoader for OfficialLoader {
    fn create_environment_fn(&self) -> Result<CreateEnvironmentFn, LoadError> {
        OfficialLoader::create_environment_fn(self)
    }

    fn runtime_version(&self) -> Result<String, LoadError> {
        OfficialLoader::runtime_version(self)
    }
}

impl Drop for OfficialLoader {
    fn drop(&mut self) {
        // SAFETY: `module` came from LoadLibraryW and is freed exactly once, here.
        unsafe { FreeLibrary(self.module) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test double for code that depends on `RuntimeLoader` rather than on the real DLL.
    struct StubLoader;

    impl RuntimeLoader for StubLoader {
        fn create_environment_fn(&self) -> Result<CreateEnvironmentFn, LoadError> {
            Err(LoadError::MissingExport(
                "CreateCoreWebView2EnvironmentWithOptions",
            ))
        }

        fn runtime_version(&self) -> Result<String, LoadError> {
            Ok("155.0.4283.62".into())
        }
    }

    #[test]
    fn runtime_loader_is_usable_as_a_trait_object() {
        let loader: &dyn RuntimeLoader = &StubLoader;
        assert_eq!(loader.runtime_version().unwrap(), "155.0.4283.62");
        assert_eq!(
            loader.create_environment_fn().err(),
            Some(LoadError::MissingExport(
                "CreateCoreWebView2EnvironmentWithOptions"
            ))
        );
    }

    #[test]
    fn missing_file_is_reported_with_path() {
        let err =
            OfficialLoader::load(Path::new(r"C:\definitely\not\here\WebView2Loader.dll")).err();
        assert!(matches!(err, Some(LoadError::NotFound(_))));
    }

    /// Needs a real loader. Set `TPT_WEBVIEW2_LOADER` to the path of `WebView2Loader.dll`.
    /// Skipped (with a message) when the variable is not set.
    #[test]
    fn loads_loader_and_reads_runtime_version() {
        let Some(path) = std::env::var_os("TPT_WEBVIEW2_LOADER") else {
            eprintln!("skipped: set TPT_WEBVIEW2_LOADER to run this test");
            return;
        };
        let loader = OfficialLoader::load(Path::new(&path))
            .expect("loader should load and export all entry points");
        let version = loader
            .runtime_version()
            .expect("runtime version should be readable");
        eprintln!("runtime version reported by loader: {version}");
        assert!(
            version.split('.').count() == 4 && version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "unexpected version string: {version}"
        );
    }
}
