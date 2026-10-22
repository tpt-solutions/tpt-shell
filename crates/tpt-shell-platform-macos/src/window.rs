// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An AppKit window that hosts one WKWebView, with the page bridge, navigation policy and
//! `app://` scheme handler attached (spec §6.2, §9).

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL};
use objc2_web_kit::{
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView,
    WKWebViewConfiguration,
};
use tpt_shell_core::backend::{AssetSource, CloseAction, MessageHandler};

use crate::policy::is_app_url;
use crate::shim::SHIM_JS;
use crate::webview::{
    evaluate, NavigationDelegate, SchemeHandler, ScriptMessageHandler, WindowDelegate, APP_SCHEME,
    BRIDGE_JS, MESSAGE_HANDLER_NAME,
};

/// The window style for a titled window, and the one used when frameless.
fn titled_style() -> NSWindowStyleMask {
    NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable
}

fn frameless_style() -> NSWindowStyleMask {
    NSWindowStyleMask::Borderless | NSWindowStyleMask::Resizable
}

define_class!(
    /// An `NSWindow` that can take key and main status even without a title bar. Without this a
    /// borderless window never becomes key, so its web view gets no keyboard input.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptWindow"]
    pub struct TptWindow;

    impl TptWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            true
        }
    }
);

/// A top-level window with a web view filling its content area.
///
/// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
pub struct MacWindow {
    window: Retained<NSWindow>,
    web: Retained<WKWebView>,
    messages: Retained<ScriptMessageHandler>,
    // WebKit holds the navigation delegate weakly, so this field keeps it alive.
    _navigation: Retained<NavigationDelegate>,
    scheme: Retained<SchemeHandler>,
    close: Retained<WindowDelegate>,
}

impl MacWindow {
    /// Creates a hidden window of `width` by `height` points, whose navigations are limited to
    /// the app origin and `allowlist`. Must run on the main thread.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn new(
        mtm: MainThreadMarker,
        title: &str,
        width: f64,
        height: f64,
        allowlist: Vec<String>,
    ) -> Self {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        // SAFETY: the window is created and used only on the main thread, as AppKit requires.
        let window: Retained<NSWindow> = unsafe {
            let this = TptWindow::alloc(mtm).set_ivars(());
            let window: Retained<TptWindow> = msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: titled_style(),
                backing: NSBackingStoreType::Buffered,
                defer: false,
            ];
            Retained::into_super(window)
        };
        // Our reference is the only one: close must not release the window a second time.
        // SAFETY: main thread; the window is kept alive by the returned struct.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));

        let close = WindowDelegate::new(mtm);
        window.setDelegate(Some(ProtocolObject::from_ref(&*close)));

        let messages = ScriptMessageHandler::new(mtm);
        let navigation = NavigationDelegate::new(mtm, allowlist);
        let scheme = SchemeHandler::new(mtm);

        // SAFETY: main thread only. The configuration is complete before the web view copies it.
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        let controller: Retained<WKUserContentController> =
            unsafe { config.userContentController() };
        // SAFETY: as above; the scripts and handlers are retained by the controller afterwards.
        unsafe {
            for source in [BRIDGE_JS, SHIM_JS] {
                let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                    WKUserScript::alloc(mtm),
                    &NSString::from_str(source),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                );
                controller.addUserScript(&script);
            }
            controller.addScriptMessageHandler_name(
                ProtocolObject::from_ref(&*messages),
                &NSString::from_str(MESSAGE_HANDLER_NAME),
            );
            config.setURLSchemeHandler_forURLScheme(
                Some(ProtocolObject::from_ref(&*scheme)),
                &NSString::from_str(APP_SCHEME),
            );
        }

        // SAFETY: as above; the frame and configuration are valid for the web view's life.
        let web = unsafe { WKWebView::initWithFrame_configuration(mtm.alloc(), frame, &config) };
        // SAFETY: the delegate is retained by this struct for as long as the web view lives.
        unsafe { web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation))) };
        window.setContentView(Some(&web));
        MacWindow {
            window,
            web,
            messages,
            _navigation: navigation,
            scheme,
            close,
        }
    }

    /// Loads an HTML document. The page has no base URL, so relative loads do not resolve.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn load_html(&self, html: &str) {
        let body = NSString::from_str(html);
        // SAFETY: a nil base URL is a valid argument for loadHTMLString.
        unsafe { self.web.loadHTMLString_baseURL(&body, None) };
    }

    /// Loads an `app://` URL. Other URLs are refused by the caller.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn navigate(&self, url: &str) -> Result<(), String> {
        if !is_app_url(url) {
            return Err(format!("navigation blocked: {url}"));
        }
        let text = NSString::from_str(url);
        // SAFETY: the string is a valid URL text; a nil result means it did not parse.
        let Some(nsurl) = NSURL::URLWithString(&text) else {
            return Err(format!("not a URL: {url}"));
        };
        let request = NSURLRequest::requestWithURL(&nsurl);
        // SAFETY: main thread; the request is live for the call.
        unsafe { self.web.loadRequest(&request) };
        Ok(())
    }

    /// Runs `script` in the page without waiting for it.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn eval(&self, script: &str) {
        evaluate(&self.web, script);
    }

    /// Sends `message` to the page's `message` listeners.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn post_message(&self, message: &str) {
        evaluate(&self.web, &crate::policy::deliver_script(message));
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_message_handler(&self, handler: MessageHandler) {
        self.messages.set_handler(handler);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_asset_source(&self, source: AssetSource) {
        self.scheme.set_source(source);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_close_action(&self, action: CloseAction) {
        self.close.set_close_action(action);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_title(&self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }

    /// Sets the content size in points.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_size(&self, width: f64, height: f64) {
        self.window.setContentSize(NSSize::new(width, height));
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn is_fullscreen(&self) -> bool {
        self.window
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn toggle_fullscreen(&self) {
        self.window.toggleFullScreen(None);
    }

    /// Removes or restores the title bar and border. The content keeps its size.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_frameless(&self, on: bool) {
        let style = if on {
            frameless_style()
        } else {
            titled_style()
        };
        self.window.setStyleMask(style);
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_always_on_top(&self, on: bool) {
        let level = if on {
            NSFloatingWindowLevel
        } else {
            NSNormalWindowLevel
        };
        self.window.setLevel(level);
    }

    /// Makes the window and the web view background clear, so the desktop shows where the page
    /// does not paint. Turning it off restores the opaque window and the page background.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_transparent(&self, on: bool) {
        // SAFETY: main thread; the colours are created and used at once.
        unsafe {
            if on {
                self.window.setOpaque(false);
                self.window.setBackgroundColor(Some(&NSColor::clearColor()));
                self.web
                    .setUnderPageBackgroundColor(Some(&NSColor::clearColor()));
            } else {
                self.window.setOpaque(true);
                self.window
                    .setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
                self.web.setUnderPageBackgroundColor(None);
            }
        }
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn close(&self) {
        self.window.close();
    }
}
