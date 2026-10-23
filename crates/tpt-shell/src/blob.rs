// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Blobs for the page (spec M2). A command hands a large payload out as an app URL, and the page
//! fetches it there instead of receiving it inside a JSON message. Windows only.
//!
//! The store is one per process, behind a lock, so [`put_blob`] works from any thread and the
//! app's asset source reads it on the UI thread. Each blob may name the window that made it;
//! [`remove_window_blobs`] drops those when the window closes. The total size is the app's limit
//! (`App::blob_limit`), 64 MiB by default.

use std::sync::Mutex;

use tpt_shell_core::backend::Asset;
use tpt_shell_ipc::blob::{id_from_route, BlobError, BlobStore, ROUTE_PREFIX};
use tpt_shell_platform_windows::webview2::APP_ORIGIN;

use crate::{Error, Result};

/// The total size of all blobs held at once, unless the app sets another limit.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub const LIMIT: usize = 64 * 1024 * 1024;

static STORE: Mutex<Option<BlobStore>> = Mutex::new(None);

/// Runs `f` on the store, which is created with [`LIMIT`] on first use.
fn with_store<T>(f: impl FnOnce(&mut BlobStore) -> T) -> T {
    let mut guard = STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let store = guard.get_or_insert_with(|| BlobStore::new(LIMIT));
    f(store)
}

/// Sets the total size of the blob store. Used by `App::build` with the app's limit. Blobs that
/// are already stored are kept.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn set_limit(bytes: usize) {
    with_store(|store| store.set_limit(bytes));
}

/// The total size the store allows, in bytes.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn limit() -> usize {
    with_store(|store| store.limit())
}

/// Stores `bytes` and returns the URL the page can fetch them from. The id is 128 random bits,
/// so the URL works as a capability: only a page that was given it can read the blob.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn put_blob(media_type: &str, bytes: Vec<u8>) -> Result<String> {
    put(None, media_type, bytes)
}

/// Like [`put_blob`], but the blob belongs to `window`: [`remove_window_blobs`] removes it when
/// that window closes.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn put_blob_for(window: &str, media_type: &str, bytes: Vec<u8>) -> Result<String> {
    put(Some(window), media_type, bytes)
}

fn put(owner: Option<&str>, media_type: &str, bytes: Vec<u8>) -> Result<String> {
    let id = tpt_shell_platform_windows::random::hex_id().map_err(Error::Backend)?;
    with_store(|store| store.put_owned(owner, &id, media_type, bytes)).map_err(blob_error)?;
    Ok(format!("{APP_ORIGIN}{ROUTE_PREFIX}{id}"))
}

/// Removes the blob behind `url`, as [`put_blob`] returned it. Returns whether one was removed.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn remove_blob(url: &str) -> bool {
    let Some(id) = id_from_route(url) else {
        return false;
    };
    with_store(|store| store.remove(id).is_some())
}

/// Removes every blob that `window` made with [`put_blob_for`]. Returns how many were removed.
/// Call it when the window closes, so its payloads do not outlive it.
///
/// Support: Partial. Windows only, verified. macOS and Linux do not build it yet.
pub fn remove_window_blobs(window: &str) -> usize {
    with_store(|store| store.remove_owned(window))
}

/// The asset for blob `id`, for the app's asset source. `None` when there is no such blob.
pub(crate) fn blob_asset(id: &str) -> Option<Asset> {
    with_store(|store| {
        store.get(id).map(|blob| Asset {
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
    fn a_window_s_blobs_are_removed_with_it_and_no_others() {
        let mine = put_blob_for("blob-test-window", "text/plain", b"a".to_vec()).expect("put");
        let theirs = put_blob_for("blob-test-other", "text/plain", b"b".to_vec()).expect("put");
        assert_eq!(remove_window_blobs("blob-test-window"), 1);
        assert!(!remove_blob(&mine), "already removed with its window");
        assert!(remove_blob(&theirs));
    }

    #[test]
    fn the_limit_is_the_app_limit_and_a_blob_over_it_is_refused() {
        let before = limit();
        set_limit(LIMIT);
        assert_eq!(limit(), LIMIT);
        let err = put_blob("application/octet-stream", vec![0; LIMIT + 1]).unwrap_err();
        assert!(matches!(err, Error::Backend(_)), "{err:?}");
        set_limit(before);
    }
}
