// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Asks the host for a large string, eight times in a row. The host counts the requests and times
// them. When the last reply arrives the page reports its length, last character and count.
var COUNT = 8;
var got = 0;
window.chrome.webview.addEventListener("message", function (e) {
  got++;
  if (got < COUNT) {
    window.chrome.webview.postMessage("blob");
  } else {
    window.chrome.webview.postMessage(
      "report:blob:" + e.data.length + ":" + e.data.charAt(e.data.length - 1) + ":" + got
    );
  }
});
window.chrome.webview.postMessage("blob");
