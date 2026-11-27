// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Reports the document type and the status of a request for a file that does not exist.
fetch("missing.txt").then(function (response) {
  document.title = location.origin + "|" + document.contentType + "|" + response.status;
}, function () {
  document.title = location.origin + "|" + document.contentType + "|error";
});
