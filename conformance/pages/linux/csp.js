// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Reports whether the inline script above ran. This external script runs only when the app's
// content security policy allows the app origin's scripts.
window.chrome.webview.postMessage("report:" + (window.inlineRan ? "inline-ran" : "inline-blocked") + "|external");
