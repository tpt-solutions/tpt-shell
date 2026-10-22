// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! One running copy of the app per bundle identifier. A second launch finds the first through
//! `NSRunningApplication`, hands it its command-line arguments in a distributed notification
//! (no sockets), brings the first to the front and reports that it should exit.
//!
//! Needs a bundle identifier, so it works for a `.app` bundle and returns
//! [`Error::Unsupported`] for a bare binary. Two copies launched in the same instant can both
//! see no other copy; that race is not closed here.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_foundation::{
    NSArray, NSBundle, NSDictionary, NSDistributedNotificationCenter, NSNotification,
    NSObjectProtocol, NSOperationQueue, NSString,
};
use tpt_shell_core::{Error, Result};

/// The key of the argument list in the notification's user info.
const ARGS_KEY: &str = "args";

/// What [`acquire`] decided.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub enum Instance {
    /// This is the first copy. Keep the guard alive for as long as the app runs.
    Primary(Primary),
    /// Another copy is running and has the arguments. This process should exit.
    Forwarded,
}

/// Keeps the listener for later launches registered. Dropping it stops listening.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct Primary {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl Drop for Primary {
    fn drop(&mut self) {
        let center = NSDistributedNotificationCenter::defaultCenter();
        // SAFETY: the token came from addObserverForName on this center and is removed once.
        unsafe {
            let token: &AnyObject =
                &*(&*self.token as *const ProtocolObject<dyn NSObjectProtocol> as *const AnyObject);
            center.removeObserver(token);
        }
    }
}

/// The notification name for a bundle identifier.
fn notification_name(bundle_id: &str) -> Retained<NSString> {
    NSString::from_str(&format!("nz.co.tpt-shell.second-instance.{bundle_id}"))
}

/// Claims the single instance for this app's bundle identifier. When this is the first copy,
/// `on_second` runs on the main thread with the arguments of each later launch. When another
/// copy is running, `args` are sent to it, it is activated, and [`Instance::Forwarded`] is
/// returned.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub fn acquire(args: &[String], on_second: impl Fn(Vec<String>) + 'static) -> Result<Instance> {
    let bundle_id = NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|s| s.to_string())
        .ok_or(Error::Unsupported {
            feature: "single instance without a bundle identifier",
        })?;
    let name = notification_name(&bundle_id);
    let center = NSDistributedNotificationCenter::defaultCenter();

    let me = std::process::id() as i32;
    let others = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(&bundle_id),
    );
    if let Some(other) = others.iter().find(|app| app.processIdentifier() != me) {
        let strings: Vec<Retained<NSString>> = args.iter().map(|a| NSString::from_str(a)).collect();
        let array = NSArray::from_retained_slice(&strings);
        let key = NSString::from_str(ARGS_KEY);
        let value: &AnyObject = &array;
        let info = NSDictionary::from_slices(&[&*key], &[value]);
        // SAFETY: only the key type parameter differs; the keys are strings either way.
        let info = unsafe { Retained::cast_unchecked::<NSDictionary>(info) };
        // SAFETY: the name and user info are plain property-list objects, as distributed
        // notifications require.
        unsafe {
            center.postNotificationName_object_userInfo_deliverImmediately(
                &name,
                None,
                Some(&info),
                true,
            );
        }
        #[allow(deprecated)] // the replacement, `activate`, needs macOS 14.
        other.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        return Ok(Instance::Forwarded);
    }

    let block = RcBlock::new(move |note: NonNull<NSNotification>| {
        // SAFETY: the center passes a live notification for the length of the callback.
        let note = unsafe { note.as_ref() };
        let args = note
            .userInfo()
            .and_then(|info| info.objectForKey(&*NSString::from_str(ARGS_KEY)))
            .and_then(|v| v.downcast::<NSArray>().ok())
            .map(|array| {
                array
                    .iter()
                    .filter_map(|item| item.downcast::<NSString>().ok())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        on_second(args);
    });
    let queue = NSOperationQueue::mainQueue();
    // SAFETY: the block is copied by the center and runs on the main queue.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(&name), None, Some(&queue), &block)
    };
    Ok(Instance::Primary(Primary { token }))
}
