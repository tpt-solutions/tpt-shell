// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Answers an `app://` request with a real HTTP response: a status code, a content type and the
//! page's Content-Security-Policy header. This is the Linux half of the policy every other
//! backend sends (spec §9).
//!
//! Uses `webkit_uri_scheme_response_*` and `webkit_uri_scheme_request_finish_with_response`
//! (WebKit2GTK 2.36 and later), and libsoup for the header block. Everything is bound at run time
//! through [`SchemeResponse::bind`]. If any symbol or libsoup is missing, `bind` fails and the
//! adapter keeps answering with the older calls, so an old WebKitGTK still starts.
//!
//! Ownership, as WebKit's source has it: the response is created with a reference the caller
//! owns and frees; WebKit takes its own reference to the response and to the stream; WebKit takes
//! ownership of the headers and frees them. So the caller unrefs the response and the stream
//! after finishing, and never frees the headers.

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};

use crate::loader::{LoadError, SharedLibrary};

/// The policy every app page is served with on Linux. Same as Windows and macOS, except that the
/// blob channel answers from its own host (`app://__tpt`), which is a different origin from the
/// app, so `connect-src` allows it by name.
///
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub const PAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
img-src 'self' data:; connect-src 'self' app://__tpt; font-src 'self'; object-src 'none'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// The libsoup that WebKit2GTK 4.1 and WebKitGTK 6.0 use (libsoup 3).
///
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub const SOUP_LIB: &[&str] = &["libsoup-3.0.so.0"];

/// `SoupMessageHeadersType::SOUP_MESSAGE_HEADERS_RESPONSE`. WebKit refuses other types.
///
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
const SOUP_MESSAGE_HEADERS_RESPONSE: c_int = 1;

/// The bound response functions. Keep this alive for as long as the scheme is registered: the
/// function pointers come from the libraries it holds.
///
/// Support: Partial. Not run on macOS/Linux here; compile-checked.
pub struct SchemeResponse {
    new: unsafe extern "C" fn(*mut c_void, i64) -> *mut c_void,
    set_status: unsafe extern "C" fn(*mut c_void, c_uint, *const c_char),
    set_content_type: unsafe extern "C" fn(*mut c_void, *const c_char),
    set_http_headers: unsafe extern "C" fn(*mut c_void, *mut c_void),
    finish_with_response: unsafe extern "C" fn(*mut c_void, *mut c_void),
    headers_new: unsafe extern "C" fn(c_int) -> *mut c_void,
    headers_append: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char),
    object_unref: unsafe extern "C" fn(*mut c_void),
    /// Held so the libraries above stay loaded. Declared last, so it is moved in after the
    /// symbols are read.
    _soup: SharedLibrary,
}

impl SchemeResponse {
    /// Binds the functions from `webkit` (the WebKit library the adapter already opened),
    /// `gobject` (for `g_object_unref`) and libsoup.
    ///
    /// # Errors
    /// A [`LoadError`] naming the first library or symbol that is missing. Callers treat that as
    /// "no response API" and keep the older answer path.
    ///
    /// Support: Partial. Not run on macOS/Linux here; compile-checked.
    pub fn bind(webkit: &SharedLibrary, gobject: &SharedLibrary) -> Result<Self, LoadError> {
        let soup = SharedLibrary::open(SOUP_LIB)?;
        // SAFETY: each prototype is the C declaration of the named function in the library
        // named, as the WebKitGTK and libsoup 3 headers declare it.
        unsafe {
            Ok(SchemeResponse {
                new: webkit.symbol("webkit_uri_scheme_response_new")?,
                set_status: webkit.symbol("webkit_uri_scheme_response_set_status")?,
                set_content_type: webkit.symbol("webkit_uri_scheme_response_set_content_type")?,
                set_http_headers: webkit.symbol("webkit_uri_scheme_response_set_http_headers")?,
                finish_with_response: webkit
                    .symbol("webkit_uri_scheme_request_finish_with_response")?,
                headers_new: soup.symbol("soup_message_headers_new")?,
                headers_append: soup.symbol("soup_message_headers_append")?,
                object_unref: gobject.symbol("g_object_unref")?,
                _soup: soup,
            })
        }
    }

    /// Answers `request` with `stream`, which holds `len` bytes (-1 if unknown), with HTTP
    /// `status`, `content_type`, and the page policy when `csp` is set.
    ///
    /// The caller keeps its reference to `stream` and must unref it after this returns. The
    /// response this creates is unreffed here.
    ///
    /// # Safety
    /// `request` is the scheme request, valid during its callback and not yet answered. `stream`
    /// is a live `GInputStream`. `content_type` is a valid C string.
    ///
    /// Support: Partial. Not run on macOS/Linux here; compile-checked.
    pub unsafe fn finish(
        &self,
        request: *mut c_void,
        stream: *mut c_void,
        len: i64,
        content_type: &CStr,
        status: u32,
        csp: bool,
    ) {
        // SAFETY: the stream is live (caller contract); WebKit takes its own reference to it.
        let response = unsafe { (self.new)(stream, len) };
        // SAFETY: `response` is the object just created; a NULL reason makes WebKit name the
        // status itself.
        unsafe {
            (self.set_status)(response, status, std::ptr::null());
            (self.set_content_type)(response, content_type.as_ptr());
        }
        if csp {
            // SAFETY: the header object is made here and handed to WebKit, which frees it; the
            // name and value are C strings that live through the calls.
            unsafe {
                let headers = (self.headers_new)(SOUP_MESSAGE_HEADERS_RESPONSE);
                if let (Ok(name), Ok(value)) = (
                    CString::new("Content-Security-Policy"),
                    CString::new(PAGE_CSP),
                ) {
                    (self.headers_append)(headers, name.as_ptr(), value.as_ptr());
                }
                (self.set_http_headers)(response, headers);
            }
        }
        // SAFETY: the request is answered exactly once, here. WebKit holds its own reference to
        // the response, so ours is released now.
        unsafe {
            (self.finish_with_response)(request, response);
            (self.object_unref)(response);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PAGE_CSP;

    #[test]
    fn the_page_policy_blocks_inline_script_and_allows_the_app_origin() {
        assert!(PAGE_CSP.contains("script-src 'self'"));
        assert!(PAGE_CSP.contains("style-src 'self'"));
        assert!(!PAGE_CSP.contains("unsafe-inline"));
        assert!(PAGE_CSP.contains("connect-src 'self' app://__tpt"));
    }
}
