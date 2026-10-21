// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Objective-C classes the WKWebView layer needs: the script message handler, the navigation
//! delegate, the `app` scheme handler and the window's close delegate. Declared with
//! `objc2::define_class!`. Every one is used on the main thread only.

use std::cell::{Cell, RefCell};

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSWindow, NSWindowDelegate};
use objc2_foundation::{
    NSData, NSDictionary, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSString, NSURL,
};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKScriptMessage,
    WKScriptMessageHandler, WKURLSchemeHandler, WKURLSchemeTask, WKUserContentController,
    WKWebView,
};
use tpt_shell_core::backend::{AssetSource, CloseAction, MessageHandler};

use crate::policy::{app_path, deliver_script, navigation_allowed, response_headers};

/// Page-side half of the message channel. Injected at document start, before the shell's
/// runtime (`js/runtime/tpt-shell.js`). It gives the runtime a `window.chrome.webview` with the
/// same shape as WebView2: `postMessage` to the shell, and a `message` listener for messages
/// from it.
pub const BRIDGE_JS: &str = "(function () {\
  if (window.chrome && window.chrome.webview) { return; }\
  var listeners = [];\
  window.chrome = window.chrome || {};\
  window.chrome.webview = {\
    postMessage: function (m) { window.webkit.messageHandlers.tptShell.postMessage(String(m)); },\
    addEventListener: function (type, fn) { if (type === 'message') { listeners.push(fn); } }\
  };\
  window.__tptDeliver = function (data) {\
    var ev = { data: data };\
    listeners.slice().forEach(function (fn) { fn(ev); });\
  };\
})();";

/// Name of the message handler the page posts to: `window.webkit.messageHandlers.tptShell`.
pub const MESSAGE_HANDLER_NAME: &str = "tptShell";

/// URL scheme the app's pages are served from.
pub const APP_SCHEME: &str = "app";

/// Runs `script` in the page. Errors the script throws are not reported.
pub fn evaluate(web: &WKWebView, script: &str) {
    let source = NSString::from_str(script);
    // SAFETY: main thread; a nil completion handler is allowed.
    unsafe { web.evaluateJavaScript_completionHandler(&source, None) };
}

/// The URL as text, or an empty string when there is none.
fn url_text(url: Option<Retained<NSURL>>) -> String {
    url.and_then(|u| u.absoluteString())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

pub struct MessageIvars {
    handler: RefCell<Option<MessageHandler>>,
}

define_class!(
    /// Receives `window.webkit.messageHandlers.tptShell.postMessage(...)` from the page.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptScriptMessageHandler"]
    #[ivars = MessageIvars]
    pub struct ScriptMessageHandler;

    unsafe impl NSObjectProtocol for ScriptMessageHandler {}

    unsafe impl WKScriptMessageHandler for ScriptMessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            // SAFETY: WebKit calls this on the main thread with a live message.
            let body = unsafe { message.body() };
            let Some(text) = body.downcast_ref::<NSString>() else {
                return;
            };
            let text = text.to_string();
            // Take the handler out while it runs, so a handler that sets a new one does not hit
            // a borrow conflict. Put it back unless it was replaced.
            let Some(mut handler) = self.ivars().handler.borrow_mut().take() else {
                return;
            };
            let reply = handler(&text);
            {
                let mut slot = self.ivars().handler.borrow_mut();
                if slot.is_none() {
                    *slot = Some(handler);
                }
            }
            if let Some(reply) = reply {
                // SAFETY: the message is live for the duration of this callback.
                if let Some(web) = unsafe { message.webView() } {
                    evaluate(&web, &deliver_script(&reply));
                }
            }
        }
    }
);

impl ScriptMessageHandler {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MessageIvars {
            handler: RefCell::new(None),
        });
        // SAFETY: NSObject's init, on a freshly allocated instance of this class.
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_handler(&self, handler: MessageHandler) {
        *self.ivars().handler.borrow_mut() = Some(handler);
    }
}

pub struct NavigationIvars {
    allowlist: RefCell<Vec<String>>,
}

define_class!(
    /// Decides, for each navigation in any frame, whether WebKit may load it (spec §9).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptNavigationDelegate"]
    #[ivars = NavigationIvars]
    pub struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {}

    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _web: &WKWebView,
            action: &WKNavigationAction,
            decision_handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            // SAFETY: WebKit calls this on the main thread with a live action.
            let url = url_text(unsafe { action.request() }.URL());
            let allowed = navigation_allowed(&url, &self.ivars().allowlist.borrow());
            let policy = if allowed {
                WKNavigationActionPolicy::Allow
            } else {
                WKNavigationActionPolicy::Cancel
            };
            decision_handler.call((policy,));
        }
    }
);

impl NavigationDelegate {
    pub fn new(mtm: MainThreadMarker, allowlist: Vec<String>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavigationIvars {
            allowlist: RefCell::new(allowlist),
        });
        // SAFETY: NSObject's init, on a freshly allocated instance of this class.
        unsafe { msg_send![super(this), init] }
    }
}

pub struct SchemeIvars {
    source: RefCell<Option<AssetSource>>,
}

define_class!(
    /// Answers requests for `app://` URLs from the window's asset source, with the page policy
    /// on every answer. Requests are answered inside `start`, so no task outlives its stop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptSchemeHandler"]
    #[ivars = SchemeIvars]
    pub struct SchemeHandler;

    unsafe impl NSObjectProtocol for SchemeHandler {}

    unsafe impl WKURLSchemeHandler for SchemeHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, _web: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            self.answer(task);
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _web: &WKWebView, _task: &ProtocolObject<dyn WKURLSchemeTask>) {}
    }
);

impl SchemeHandler {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SchemeIvars {
            source: RefCell::new(None),
        });
        // SAFETY: NSObject's init, on a freshly allocated instance of this class.
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_source(&self, source: AssetSource) {
        *self.ivars().source.borrow_mut() = Some(source);
    }

    /// Answers `task` with the asset its URL names, or 404 when there is none.
    fn answer(&self, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        // SAFETY: the task is live while WebKit calls us.
        let request = unsafe { task.request() };
        let Some(url) = request.URL() else {
            return;
        };
        let text = url_text(Some(url.clone()));
        let found = app_path(&text).and_then(|path| {
            let source = self.ivars().source.borrow();
            source.as_ref().and_then(|find| find(&path))
        });
        let (status, content_type, body) = match found {
            Some(asset) => (200, asset.content_type, asset.body),
            None => (404, "text/plain".to_string(), Vec::new()),
        };

        let headers = response_headers(&content_type);
        let keys: Vec<Retained<NSString>> =
            headers.iter().map(|(k, _)| NSString::from_str(k)).collect();
        let values: Vec<Retained<NSString>> =
            headers.iter().map(|(_, v)| NSString::from_str(v)).collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let value_refs: Vec<&NSString> = values.iter().map(|v| &**v).collect();
        let header_dict = NSDictionary::from_slices(&key_refs, &value_refs);
        let version = NSString::from_str("HTTP/1.1");

        let response = {
            NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                status,
                Some(&version),
                Some(&header_dict),
            )
        };
        let Some(response) = response else {
            return;
        };
        let data = NSData::with_bytes(&body);
        // SAFETY: called once for this live task, from the callback WebKit made for it.
        unsafe {
            task.didReceiveResponse(&response);
            task.didReceiveData(&data);
            task.didFinish();
        }
    }
}

pub struct WindowDelegateIvars {
    close_action: Cell<CloseAction>,
}

define_class!(
    /// Handles the user's close request for one window (title bar button, Cmd+W).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TptWindowDelegate"]
    #[ivars = WindowDelegateIvars]
    pub struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, sender: &NSWindow) -> bool {
            match self.ivars().close_action.get() {
                CloseAction::Close => true,
                CloseAction::Hide => {
                    sender.orderOut(None);
                    false
                }
            }
        }
    }
);

impl WindowDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(WindowDelegateIvars {
            close_action: Cell::new(CloseAction::Close),
        });
        // SAFETY: NSObject's init, on a freshly allocated instance of this class.
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_close_action(&self, action: CloseAction) {
        self.ivars().close_action.set(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_script_defines_the_webview2_shape() {
        assert!(BRIDGE_JS.contains("window.chrome.webview"));
        assert!(BRIDGE_JS.contains("messageHandlers.tptShell.postMessage"));
        assert!(BRIDGE_JS.contains("__tptDeliver"));
    }
}
