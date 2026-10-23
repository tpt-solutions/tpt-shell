// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Secrets in the user's login keychain, through the Security framework's generic password
//! items (`SecItemAdd`, `SecItemCopyMatching`, `SecItemUpdate`, `SecItemDelete`).
//!
//! The query dictionaries are built as `NSDictionary`, which is toll-free bridged with
//! `CFDictionary`, so no CoreFoundation container code is needed. Items are marked
//! `WhenUnlockedThisDeviceOnly`: they do not sync and are unreadable while the Mac is locked.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::Message;
use objc2_core_foundation::{CFDictionary, CFString, CFType};
use objc2_foundation::{NSData, NSDictionary, NSNumber, NSString};
use objc2_security::{
    errSecItemNotFound, errSecSuccess, kSecAttrAccessible,
    kSecAttrAccessibleWhenUnlockedThisDeviceOnly, kSecAttrAccount, kSecAttrService, kSecClass,
    kSecClassGenericPassword, kSecMatchLimit, kSecMatchLimitOne, kSecReturnData, kSecValueData,
    SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate,
};
use tpt_shell_core::{Error, Result};

/// A keychain namespace. Every item is stored under this service name.
///
/// Support: Partial. Not run on macOS here; compile-checked against the Security bindings.
pub struct Keystore {
    service: Retained<NSString>,
}

/// A Security framework constant as the `NSString` it is bridged with.
fn constant(value: &'static CFString) -> &'static NSString {
    // SAFETY: CFString and NSString are toll-free bridged, so the pointer is a valid NSString
    // for as long as the constant lives, which is the whole process.
    unsafe { &*(value as *const CFString as *const NSString) }
}

/// A query or attribute dictionary as the `CFDictionary` the Security calls take.
fn bridged(dict: &NSDictionary<NSString, AnyObject>) -> &CFDictionary {
    // SAFETY: NSDictionary is toll-free bridged with CFDictionary.
    unsafe { &*(dict as *const NSDictionary<NSString, AnyObject> as *const CFDictionary) }
}

fn status_error(what: &str, status: i32) -> Error {
    Error::Backend(format!("keychain {what} failed (OSStatus {status})"))
}

impl Keystore {
    /// A keystore whose items are filed under `service`, such as the app's bundle id.
    ///
    /// Support: Partial. Security framework generic-password items. Not run on macOS here;
    /// compile-checked against the Security bindings.
    pub fn new(service: &str) -> Self {
        Keystore {
            service: NSString::from_str(service),
        }
    }

    /// The query that names one item: class, service and account.
    fn query(
        &self,
        name: &str,
    ) -> (
        Retained<NSString>,
        Vec<(&'static NSString, Retained<AnyObject>)>,
    ) {
        let account = NSString::from_str(name);
        // SAFETY: the Security constants are valid for the process lifetime.
        let items = unsafe {
            vec![
                (
                    constant(kSecClass),
                    Retained::cast_unchecked::<AnyObject>(
                        constant(kSecClassGenericPassword).retain(),
                    ),
                ),
                (
                    constant(kSecAttrService),
                    Retained::cast_unchecked::<AnyObject>(self.service.clone()),
                ),
                (
                    constant(kSecAttrAccount),
                    Retained::cast_unchecked::<AnyObject>(account.clone()),
                ),
            ]
        };
        (account, items)
    }

    /// Builds a dictionary from `items` plus `extra`.
    fn dictionary(
        items: &[(&'static NSString, Retained<AnyObject>)],
        extra: &[(&'static NSString, Retained<AnyObject>)],
    ) -> Retained<NSDictionary<NSString, AnyObject>> {
        let keys: Vec<&NSString> = items.iter().chain(extra).map(|(k, _)| *k).collect();
        let values: Vec<&AnyObject> = items.iter().chain(extra).map(|(_, v)| &**v).collect();
        NSDictionary::from_slices(&keys, &values)
    }

    /// Stores `secret` under `name`, replacing an earlier value.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the Security bindings.
    pub fn set(&self, name: &str, secret: &[u8]) -> Result<()> {
        let (_account, base) = self.query(name);
        let data = NSData::with_bytes(secret);
        // SAFETY: the constants are valid for the process lifetime; the data is an NSData.
        let value_key = constant(unsafe { kSecValueData });
        let data_obj = unsafe { Retained::cast_unchecked::<AnyObject>(data) };

        // Update first; add when there is nothing to update.
        let query = Self::dictionary(&base, &[]);
        let update = Self::dictionary(&[], &[(value_key, data_obj.clone())]);
        // SAFETY: both dictionaries are valid bridged CFDictionaries for the call.
        let status = unsafe { SecItemUpdate(bridged(&query), bridged(&update)) };
        if status == errSecSuccess {
            return Ok(());
        }
        if status != errSecItemNotFound {
            return Err(status_error("update", status));
        }
        // SAFETY: the constants are valid for the process lifetime.
        let access_key = constant(unsafe { kSecAttrAccessible });
        let access_val = unsafe {
            Retained::cast_unchecked::<AnyObject>(
                constant(kSecAttrAccessibleWhenUnlockedThisDeviceOnly).retain(),
            )
        };
        let add = Self::dictionary(&base, &[(value_key, data_obj), (access_key, access_val)]);
        // SAFETY: the dictionary is valid for the call; a null result pointer is allowed.
        let status = unsafe { SecItemAdd(bridged(&add), std::ptr::null_mut()) };
        if status == errSecSuccess {
            Ok(())
        } else {
            Err(status_error("add", status))
        }
    }

    /// The secret stored under `name`, or `None`.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the Security bindings.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let (_account, base) = self.query(name);
        let yes = NSNumber::new_bool(true);
        // SAFETY: the constants are valid for the process lifetime.
        let extra = unsafe {
            [
                (
                    constant(kSecReturnData),
                    Retained::cast_unchecked::<AnyObject>(yes),
                ),
                (
                    constant(kSecMatchLimit),
                    Retained::cast_unchecked::<AnyObject>(constant(kSecMatchLimitOne).retain()),
                ),
            ]
        };
        let query = Self::dictionary(&base, &extra);
        let mut result: *const CFType = std::ptr::null();
        // SAFETY: the query is valid for the call; on success `result` is a +1 retained CFData.
        let status = unsafe { SecItemCopyMatching(bridged(&query), &mut result) };
        if status == errSecItemNotFound {
            return Ok(None);
        }
        if status != errSecSuccess {
            return Err(status_error("read", status));
        }
        // SAFETY: a successful query with kSecReturnData returns an owned CFData, bridged to
        // NSData; taking ownership releases it when dropped.
        let data = unsafe { Retained::from_raw(result as *mut NSData) }
            .ok_or_else(|| Error::Backend("keychain returned no data".into()))?;
        Ok(Some(data.to_vec()))
    }

    /// Removes the secret stored under `name`. Returns whether there was one.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the Security bindings.
    pub fn delete(&self, name: &str) -> Result<bool> {
        let (_account, base) = self.query(name);
        let query = Self::dictionary(&base, &[]);
        // SAFETY: the query is a valid bridged CFDictionary for the call.
        let status = unsafe { SecItemDelete(bridged(&query)) };
        match status {
            s if s == errSecSuccess => Ok(true),
            s if s == errSecItemNotFound => Ok(false),
            s => Err(status_error("delete", s)),
        }
    }
}
