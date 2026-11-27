// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Tries to leave the app origin. If the attempt is refused, this page is still here after two
// seconds and reports its origin. If the attempt succeeds, the timer never runs and nothing is
// reported.
setTimeout(function () {
  location.href = "https://example.com/";
}, 100);
setTimeout(function () {
  window.chrome.webview.postMessage("report:" + location.origin);
}, 2000);
