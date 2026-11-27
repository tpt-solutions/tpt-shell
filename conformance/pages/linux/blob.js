// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Asks the host for a large string, eight times in a row. The host counts the requests and times
// them. The page only asks again after each reply, so the count is the check.
var COUNT = 8;
var got = 0;
window.chrome.webview.addEventListener("message", function (e) {
  got++;
  if (got < COUNT) {
    window.chrome.webview.postMessage("blob");
  }
});
window.chrome.webview.postMessage("blob");
