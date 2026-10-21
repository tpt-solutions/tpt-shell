// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An AppKit window that hosts one WKWebView.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

/// A top-level window with a web view filling its content area.
pub struct MacWindow {
    window: Retained<NSWindow>,
    web: Retained<WKWebView>,
}

impl MacWindow {
    /// Creates a hidden window of `width` by `height` points. Must run on the main thread.
    pub fn new(mtm: MainThreadMarker, title: &str, width: f64, height: f64) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: the window is created and used only on the main thread, as AppKit requires.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(title));

        // SAFETY: main-thread only, like the rest of AppKit here.
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        // SAFETY: as above; the configuration object is owned by the web view afterwards.
        let web = unsafe { WKWebView::initWithFrame_configuration(mtm.alloc(), frame, &config) };
        window.setContentView(Some(&web));
        MacWindow { window, web }
    }

    /// Loads an HTML document. The page has no base URL, so relative loads do not resolve.
    pub fn load_html(&self, html: &str) {
        let body = NSString::from_str(html);
        // SAFETY: a nil base URL is a valid argument for loadHTMLString.
        unsafe { self.web.loadHTMLString_baseURL(&body, None) };
    }

    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    pub fn set_title(&self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }
}

impl MacWindow {
    /// Sets the content size in points.
    pub fn set_size(&self, width: f64, height: f64) {
        self.window.setContentSize(NSSize::new(width, height));
    }

    pub fn is_fullscreen(&self) -> bool {
        self.window
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
    }

    pub fn toggle_fullscreen(&self) {
        self.window.toggleFullScreen(None);
    }

    pub fn close(&self) {
        self.window.close();
    }
}
