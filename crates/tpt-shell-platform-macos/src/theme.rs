// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Appearance observation. macOS posts `AppleInterfaceThemeChangedNotification` on the
//! distributed notification center when the user switches between light and dark. This
//! module turns that into a callback on the main thread.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSObjectProtocol, NSOperationQueue, NSString,
    NSUserDefaults,
};
use tpt_shell_core::platform::ColorScheme;

/// The scheme the user chose in System Settings. The global `AppleInterfaceStyle` default is
/// `Dark` in dark mode and absent in light mode. Unlike the app's effective appearance, it is
/// already updated when the change notification arrives.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub fn system_scheme() -> ColorScheme {
    let style = NSUserDefaults::standardUserDefaults()
        .stringForKey(&NSString::from_str("AppleInterfaceStyle"));
    match style {
        Some(s) if s.to_string() == "Dark" => ColorScheme::Dark,
        _ => ColorScheme::Light,
    }
}

/// Keeps an appearance callback registered. Dropping it unregisters the callback.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct AppearanceObserver {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl Drop for AppearanceObserver {
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

/// Calls `on_change` on the main thread with the new scheme each time the user switches the
/// system between light and dark.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub fn observe_appearance(on_change: impl Fn(ColorScheme) + 'static) -> AppearanceObserver {
    let center = NSDistributedNotificationCenter::defaultCenter();
    let name = NSString::from_str("AppleInterfaceThemeChangedNotification");
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        on_change(system_scheme());
    });
    let queue = NSOperationQueue::mainQueue();
    // SAFETY: the block is copied by the center, and it runs on the main queue, where the
    // callback was created.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(&name), None, Some(&queue), &block)
    };
    AppearanceObserver { token }
}
