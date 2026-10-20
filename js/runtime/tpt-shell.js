// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// tpt-shell runtime (spec §7.2). Injected at document start by the backend.
// Plain JavaScript, no build step. Sends IPC v1 envelopes over chrome.webview.
(function () {
  if (window.tpt || !window.chrome || !window.chrome.webview) {
    return;
  }

  var nextId = 1;
  var pending = new Map();

  window.chrome.webview.addEventListener('message', function (event) {
    var msg;
    try {
      msg = typeof event.data === 'string' ? JSON.parse(event.data) : event.data;
    } catch (e) {
      return;
    }
    // Events from Rust carry no id match here and are ignored until `tpt.on` exists.
    var entry = pending.get(msg && msg.id);
    if (!entry) {
      return;
    }
    pending.delete(msg.id);
    if (msg.ok) {
      entry.resolve(msg.data);
    } else {
      entry.reject(msg.error);
    }
  });

  window.tpt = {
    invoke: function (cmd, args) {
      var id = nextId++;
      return new Promise(function (resolve, reject) {
        pending.set(id, { resolve: resolve, reject: reject });
        window.chrome.webview.postMessage(
          JSON.stringify({ v: 1, id: id, cmd: cmd, args: args || {} })
        );
      });
    }
  };
})();
