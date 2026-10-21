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
  var listeners = new Map();

  window.chrome.webview.addEventListener('message', function (event) {
    var msg;
    try {
      msg = typeof event.data === 'string' ? JSON.parse(event.data) : event.data;
    } catch (e) {
      return;
    }
    // Events from Rust are {"event","data"} with no id.
    if (msg && typeof msg.event === 'string') {
      var subs = listeners.get(msg.event);
      if (subs) {
        subs.slice().forEach(function (handler) {
          try {
            handler(msg.data);
          } catch (e) {
            // Rethrow later so one bad handler does not stop the others.
            setTimeout(function () {
              throw e;
            });
          }
        });
      }
      return;
    }
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

  var edge = /Edg\/([\d.]+)/.exec(navigator.userAgent);

  window.tpt = {
    invoke: function (cmd, args) {
      var id = nextId++;
      return new Promise(function (resolve, reject) {
        pending.set(id, { resolve: resolve, reject: reject });
        window.chrome.webview.postMessage(
          JSON.stringify({ v: 1, id: id, cmd: cmd, args: args || {} })
        );
      });
    },
    on: function (name, handler) {
      var subs = listeners.get(name);
      if (!subs) {
        subs = [];
        listeners.set(name, subs);
      }
      subs.push(handler);
      return function () {
        var i = subs.indexOf(handler);
        if (i >= 0) {
          subs.splice(i, 1);
        }
      };
    },
    caps: {
      engine: 'webview2',
      version: edge ? edge[1] : null
    }
  };
})();
