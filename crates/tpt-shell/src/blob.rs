// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Blobs for the page (spec M2). A command hands a large payload out as an app URL, and the page
//! fetches it there instead of receiving it inside a JSON message. Windows only.
//!
//! The blobs live on the UI thread, in the store that the app serves from. Call [`put_blob`]
//! from a command handler or from other code on the UI thread.

use std::cell::RefCell;

use tpt_shell_core::backend::Asset;
use tpt_shell_ipc::blob::{id_from_route, BlobError, BlobStore, ROUTE_PREFIX};
use tpt_shell_platform_windows::webview2::APP_ORIGIN;

use crate::{Error, Result};

/// The total size of all blobs held at once.
pub const LIMIT: usize = 64 * 1024 * 1024;

thread_local! {
    static STORE: RefCell<BlobStore> = RefCell::new(BlobStore::new(LIMIT));
}

/// Stores `bytes` and returns the URL the page can fetch them from. The id is 128 random bits,
/// so the URL works as a capability: only a page that was given it can read the blob.
pub fn put_blob(media_type: &str, bytes: Vec<u8>) -> Result<String> {
    let id = tpt_shell_platform_windows::random::hex_id().map_err(Error::Backend)?;
    STORE
        .with(|store| store.borrow_mut().put(&id, media_type, bytes))
        .map_err(blob_error)?;
    Ok(format!("{APP_ORIGIN}{ROUTE_PREFIX}{id}"))
}

/// Removes the blob behind `url`, as [`put_blob`] returned it. Returns whether one was removed.
pub fn remove_blob(url: &str) -> bool {
    let Some(id) = id_from_route(url) else {
        return false;
    };
    STORE.with(|store| store.borrow_mut().remove(id).is_some())
}

/// The asset for blob `id`, for the app's asset source. `None` when there is no such blob.
pub(crate) fn blob_asset(id: &str) -> Option<Asset> {
    STORE.with(|store| {
        store.borrow().get(id).map(|blob| Asset {
            content_type: blob.media_type.clone(),
            body: blob.bytes.clone(),
        })
    })
}

fn blob_error(error: BlobError) -> Error {
    Error::Backend(match error {
        BlobError::BadId => "blob id is not 32 lowercase hex digits".into(),
        BlobError::Exists => "blob id is already in use".into(),
        BlobError::QuotaExceeded { used, limit } => {
            format!("blobs would use more than {limit} bytes (already {used})")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blob_is_served_under_its_url_until_it_is_removed() {
        let url = put_blob("text/plain", b"hello".to_vec()).expect("put");
        let prefix = format!("{APP_ORIGIN}{ROUTE_PREFIX}");
        let id = url
            .strip_prefix(&prefix)
            .expect("the url names the blob route");
        assert!(tpt_shell_ipc::blob::is_valid_id(id));

        let asset = blob_asset(id).expect("stored");
        assert_eq!(asset.content_type, "text/plain");
        assert_eq!(asset.body, b"hello");

        assert!(remove_blob(&url));
        assert!(blob_asset(id).is_none());
        assert!(!remove_blob(&url));
    }

    #[test]
    fn a_blob_over_the_limit_is_refused() {
        let err = put_blob("application/octet-stream", vec![0; LIMIT + 1]).unwrap_err();
        assert!(matches!(err, Error::Backend(_)), "{err:?}");
    }
}
