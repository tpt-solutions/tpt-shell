// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! An AppKit window that hosts one WKWebView, with the page bridge, navigation policy and
//! `app://` scheme handler attached (spec §6.2, §9).

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSAutoresizingMaskOptions, NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep,
    NSColor, NSFloatingWindowLevel, NSImage, NSNormalWindowLevel, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSDictionary, NSError, NSObjectProtocol, NSPoint, NSRect,
    NSRunLoop, NSSize, NSString, NSURLRequest, NSURL,
};
use objc2_web_kit::{
    WKSnapshotConfiguration, WKUserContentController, WKUserScript, WKUserScriptInjectionTime,
    WKWebView, WKWebViewConfiguration,
};
use tpt_shell_core::backend::{
    AssetSource, Backdrop, CloseAction, MessageHandler, PreferredScheme,
};

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
    // The vibrancy view while a backdrop is on. It then is the content view and holds `web`.
    effect: RefCell<Option<Retained<NSVisualEffectView>>>,
    transparent: Cell<bool>,
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
            effect: RefCell::new(None),
            transparent: Cell::new(false),
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
        self.transparent.set(on);
        // A vibrancy view needs the see-through look whatever the flag says.
        let on = on || self.effect.borrow().is_some();
        self.set_transparent_look(on);
    }

    /// The window and page background part of transparency.
    fn set_transparent_look(&self, on: bool) {
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

    /// Puts a vibrancy view behind the page, or takes it away. macOS has no Mica or Acrylic:
    /// they map to the closest `NSVisualEffectMaterial` (Mica: window background, Acrylic:
    /// HUD window, Tabbed: header view). The window and page are made see-through so the
    /// material shows where the page does not paint.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_backdrop(&self, mtm: MainThreadMarker, backdrop: Backdrop) {
        let material = match backdrop {
            Backdrop::None => None,
            Backdrop::Mica => Some(NSVisualEffectMaterial::WindowBackground),
            Backdrop::Acrylic => Some(NSVisualEffectMaterial::HUDWindow),
            Backdrop::Tabbed => Some(NSVisualEffectMaterial::HeaderView),
        };
        let mut slot = self.effect.borrow_mut();
        let Some(material) = material else {
            // Put the web view back as the content view, if a vibrancy view had it.
            if slot.take().is_some() {
                self.web.removeFromSuperview();
                self.window.setContentView(Some(&self.web));
            }
            self.set_transparent_look(self.transparent.get());
            return;
        };
        let effect = match slot.as_ref() {
            Some(existing) => existing.clone(),
            None => {
                let size = NSSize::new(self.web.frame().size.width, self.web.frame().size.height);
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), size);
                let view = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
                view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                view.setState(NSVisualEffectState::Active);
                let sizable = NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable;
                view.setAutoresizingMask(sizable);
                // The web view leaves the content view slot before it joins the vibrancy view.
                self.web.removeFromSuperview();
                self.window.setContentView(Some(&view));
                self.web.setFrame(view.bounds());
                self.web.setAutoresizingMask(sizable);
                view.addSubview(&self.web);
                *slot = Some(view.clone());
                view
            }
        };
        effect.setMaterial(material);
        self.set_transparent_look(true);
    }

    /// Sets the appearance of this window, which is what the page's `prefers-color-scheme`
    /// follows. `System` clears the override.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_preferred_scheme(&self, scheme: PreferredScheme) {
        // SAFETY: both names are constants provided by AppKit.
        let appearance: Option<Retained<NSAppearance>> = match scheme {
            PreferredScheme::System => None,
            PreferredScheme::Light => unsafe {
                NSAppearance::appearanceNamed(NSAppearanceNameAqua)
            },
            PreferredScheme::Dark => unsafe {
                NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua)
            },
        };
        self.window.setAppearance(appearance.as_deref());
    }

    /// Turns Web Inspector on or off for the page. `isInspectable` exists from macOS 13.3, so
    /// this returns false on older systems, where the request cannot be honoured.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn set_inspectable(&self, on: bool) -> bool {
        if !self.web.respondsToSelector(sel!(setInspectable:)) {
            return false;
        }
        // SAFETY: the selector exists, checked above; main thread.
        unsafe { self.web.setInspectable(on) };
        true
    }

    /// Captures the page as PNG bytes. WebKit answers on the main run loop, so this pumps that
    /// loop until the answer arrives or ten seconds pass.
    ///
    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn snapshot(&self, mtm: MainThreadMarker) -> Result<Vec<u8>, String> {
        let slot: Rc<RefCell<Option<SnapshotResult>>> = Rc::new(RefCell::new(None));
        let done = slot.clone();
        let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let result = match NonNull::new(image) {
                // SAFETY: WebKit passes a live image that stays valid for this callback.
                Some(image) => png_bytes(unsafe { image.as_ref() }),
                None => {
                    // SAFETY: as above, for the error.
                    let why = unsafe { error.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "no image".to_string());
                    Err(format!("snapshot failed: {why}"))
                }
            };
            *done.borrow_mut() = Some(result);
        });
        // SAFETY: main thread. The default configuration captures the visible page. WebKit
        // copies the block and calls it on this thread's run loop.
        unsafe {
            let config = WKSnapshotConfiguration::new(mtm);
            self.web
                .takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
        }
        let run_loop = NSRunLoop::currentRunLoop();
        for _ in 0..200 {
            if slot.borrow().is_some() {
                break;
            }
            // SAFETY: the mode is a Foundation constant; the date is made for this call.
            unsafe {
                run_loop.runMode_beforeDate(
                    NSDefaultRunLoopMode,
                    &NSDate::dateWithTimeIntervalSinceNow(0.05),
                );
            }
        }
        let result = slot.borrow_mut().take();
        result.unwrap_or_else(|| Err("snapshot timed out".to_string()))
    }

    /// Support: Partial. Not run on macOS here; compile-checked against the AppKit bindings.
    pub fn close(&self) {
        self.window.close();
    }
}

/// What a snapshot callback leaves behind: PNG bytes or the reason it failed.
type SnapshotResult = Result<Vec<u8>, String>;

/// Encodes an image as PNG through its TIFF form.
fn png_bytes(image: &NSImage) -> Result<Vec<u8>, String> {
    let tiff = image.TIFFRepresentation().ok_or("no TIFF form")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("unreadable image")?;
    let props = NSDictionary::new();
    // SAFETY: an empty properties dictionary is valid for PNG output.
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }
        .ok_or("PNG encoding failed")?;
    Ok(png.to_vec())
}
