// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Reports the origin, the document type and the status of a request for a file that does not
// exist. The Linux checks read messages, because the backend has no title query yet.
fetch("missing.txt").then(function (response) {
  window.chrome.webview.postMessage("report:" + location.origin + "|" + document.contentType + "|" + response.status);
}, function () {
  window.chrome.webview.postMessage("report:" + location.origin + "|" + document.contentType + "|error");
});
