// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Blob channel (spec M2): large binary payloads that go to the page by URL instead of through
//! the JSON envelope. This module holds the blobs and parses the route. Serving the route from
//! the app origin is the platform's job.
//!
//! Routes look like `app://__tpt/blob/<id>`. Ids are 32 lowercase hex digits, chosen by the
//! caller, so a page cannot guess or name a blob it was not given.

use std::collections::HashMap;

/// A stored payload and its media type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

/// Why a blob could not be stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobError {
    /// The id is not 32 lowercase hex digits.
    BadId,
    /// The id is already in use. Blobs are never overwritten.
    Exists,
    /// Storing this blob would pass the total size limit.
    QuotaExceeded { used: usize, limit: usize },
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlobError::BadId => write!(f, "blob ids are 32 lowercase hex digits"),
            BlobError::Exists => write!(f, "a blob with this id already exists"),
            BlobError::QuotaExceeded { used, limit } => {
                write!(f, "blob store is {used} bytes, limit is {limit}")
            }
        }
    }
}

impl std::error::Error for BlobError {}

/// Blobs held for one app, with a total size cap. Each blob may name an owner (a window), so
/// that the blobs of a window can be removed when it closes.
pub struct BlobStore {
    limit: usize,
    used: usize,
    blobs: HashMap<String, Blob>,
    owners: HashMap<String, String>,
}

impl BlobStore {
    pub fn new(limit: usize) -> Self {
        BlobStore {
            limit,
            used: 0,
            blobs: HashMap::new(),
            owners: HashMap::new(),
        }
    }

    /// Stores `bytes` under `id`, with no owner.
    pub fn put(&mut self, id: &str, media_type: &str, bytes: Vec<u8>) -> Result<(), BlobError> {
        self.put_owned(None, id, media_type, bytes)
    }

    /// Stores `bytes` under `id`, owned by `owner` (a window name) when one is given.
    pub fn put_owned(
        &mut self,
        owner: Option<&str>,
        id: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<(), BlobError> {
        if !is_valid_id(id) {
            return Err(BlobError::BadId);
        }
        if self.blobs.contains_key(id) {
            return Err(BlobError::Exists);
        }
        if self.used + bytes.len() > self.limit {
            return Err(BlobError::QuotaExceeded {
                used: self.used,
                limit: self.limit,
            });
        }
        self.used += bytes.len();
        if let Some(owner) = owner {
            self.owners.insert(id.to_string(), owner.to_string());
        }
        self.blobs.insert(
            id.to_string(),
            Blob {
                media_type: media_type.to_string(),
                bytes,
            },
        );
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Blob> {
        self.blobs.get(id)
    }

    /// Removes a blob and returns its size. Removing frees its quota.
    pub fn remove(&mut self, id: &str) -> Option<usize> {
        let blob = self.blobs.remove(id)?;
        self.owners.remove(id);
        self.used -= blob.bytes.len();
        Some(blob.bytes.len())
    }

    /// Removes every blob owned by `owner`, and returns how many were removed.
    pub fn remove_owned(&mut self, owner: &str) -> usize {
        let ids: Vec<String> = self
            .owners
            .iter()
            .filter(|(_, o)| o.as_str() == owner)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.remove(id);
        }
        ids.len()
    }

    /// The total size allowed for this store.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Changes the total size allowed. Blobs already stored are kept; a lower limit only refuses
    /// new blobs until enough are removed.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
    }

    pub fn used(&self) -> usize {
        self.used
    }
}

/// An id is exactly 32 lowercase hex digits (128 bits).
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Path prefix of blob routes, after the origin.
pub const ROUTE_PREFIX: &str = "/__tpt/blob/";

/// Parses the id out of a blob route. Accepts a path (`/__tpt/blob/<id>`) or a full URL with
/// any origin, and rejects anything with a query, a fragment or extra path segments.
pub fn id_from_route(url_or_path: &str) -> Option<&str> {
    // The app scheme names its host: app://__tpt/blob/<id>.
    if let Some(rest) = url_or_path.strip_prefix("app://") {
        let id = rest.strip_prefix("__tpt/blob/")?;
        return is_valid_id(id).then_some(id);
    }
    let path = match url_or_path.find("://") {
        Some(scheme_end) => {
            let after = &url_or_path[scheme_end + 3..];
            let slash = after.find('/')?;
            &after[slash..]
        }
        None => url_or_path,
    };
    let id = path.strip_prefix(ROUTE_PREFIX)?;
    is_valid_id(id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn put_get_and_remove() {
        let mut store = BlobStore::new(100);
        store.put(ID, "image/png", vec![1, 2, 3]).unwrap();
        assert_eq!(store.get(ID).unwrap().media_type, "image/png");
        assert_eq!(store.used(), 3);
        assert_eq!(store.remove(ID), Some(3));
        assert_eq!(store.used(), 0);
        assert!(store.get(ID).is_none());
    }

    #[test]
    fn ids_must_be_32_lowercase_hex_digits() {
        let mut store = BlobStore::new(100);
        for bad in [
            "",
            "xyz",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123456789abcdef0123456789abcde",
            "../../etc/passwd",
        ] {
            assert_eq!(store.put(bad, "x", vec![]), Err(BlobError::BadId), "{bad}");
        }
    }

    #[test]
    fn blobs_are_never_overwritten() {
        let mut store = BlobStore::new(100);
        store.put(ID, "a", vec![1]).unwrap();
        assert_eq!(store.put(ID, "b", vec![2]), Err(BlobError::Exists));
        assert_eq!(store.get(ID).unwrap().bytes, vec![1]);
    }

    #[test]
    fn the_quota_is_checked_before_storing() {
        let mut store = BlobStore::new(4);
        store.put(ID, "a", vec![0; 3]).unwrap();
        let other = "fedcba9876543210fedcba9876543210";
        assert_eq!(
            store.put(other, "a", vec![0; 2]),
            Err(BlobError::QuotaExceeded { used: 3, limit: 4 })
        );
        assert!(store.get(other).is_none());
        store.remove(ID);
        assert!(store.put(other, "a", vec![0; 2]).is_ok());
    }

    #[test]
    fn routes_yield_the_id_and_nothing_else() {
        assert_eq!(id_from_route(&format!("/__tpt/blob/{ID}")), Some(ID));
        assert_eq!(id_from_route(&format!("app://__tpt/blob/{ID}")), Some(ID));
        assert_eq!(
            id_from_route(&format!("https://tpt-app.localhost/__tpt/blob/{ID}")),
            Some(ID)
        );
        for bad in [
            format!("/__tpt/blob/{ID}?x=1"),
            format!("/__tpt/blob/{ID}/extra"),
            format!("/__tpt/blob/{ID}#frag"),
            "/__tpt/blob/short".to_string(),
            "/other/path".to_string(),
        ] {
            assert_eq!(id_from_route(&bad), None, "{bad}");
        }
    }

    const OTHER: &str = "fedcba9876543210fedcba9876543210";

    #[test]
    fn removing_an_owner_removes_only_its_blobs() {
        let mut store = BlobStore::new(100);
        store
            .put_owned(Some("main"), ID, "text/plain", vec![0; 10])
            .unwrap();
        store
            .put_owned(Some("main"), OTHER, "text/plain", vec![0; 20])
            .unwrap();
        store
            .put_owned(
                Some("aux"),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "text/plain",
                vec![0; 5],
            )
            .unwrap();
        store
            .put("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "text/plain", vec![0; 1])
            .unwrap();

        assert_eq!(store.remove_owned("main"), 2);
        assert!(store.get(ID).is_none());
        assert!(store.get(OTHER).is_none());
        assert_eq!(store.used(), 6, "freed quota comes back");
        assert!(store.get("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_some());
        assert_eq!(store.remove_owned("main"), 0);
    }

    #[test]
    fn a_removed_blob_frees_its_owner_entry() {
        let mut store = BlobStore::new(100);
        store
            .put_owned(Some("main"), ID, "text/plain", vec![0; 10])
            .unwrap();
        store.remove(ID);
        assert_eq!(store.remove_owned("main"), 0);
    }

    #[test]
    fn the_limit_can_be_read_and_changed() {
        let mut store = BlobStore::new(10);
        assert_eq!(store.limit(), 10);
        store.put(ID, "text/plain", vec![0; 8]).unwrap();
        store.set_limit(4);
        assert_eq!(store.limit(), 4);
        assert!(store.get(ID).is_some(), "stored blobs are kept");
        assert!(matches!(
            store.put(OTHER, "text/plain", vec![0; 1]),
            Err(BlobError::QuotaExceeded { .. })
        ));
    }
}
