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

  function invoke(cmd, args) {
    var id = nextId++;
    return new Promise(function (resolve, reject) {
      pending.set(id, { resolve: resolve, reject: reject });
      window.chrome.webview.postMessage(
        JSON.stringify({ v: 1, id: id, cmd: cmd, args: args || {} })
      );
    });
  }

  function on(name, handler) {
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
  }

  // A started job (spec §7.2). `cancel` calls the job.cancel command. `on` takes only the events
  // of this job, from the job.* events the runner sends.
  function job(id) {
    return {
      id: id,
      cancel: function () {
        return invoke('job.cancel', { job: id });
      },
      on: function (name, handler) {
        return on(name, function (data) {
          if (data && data.job === id) {
            handler(data);
          }
        });
      }
    };
  }

  // Tauri-style shim (spec §8). Subset: `invoke` and `event.listen`. Opt-in: nothing is set on
  // window until `install()` is called.
  var tauri = {
    invoke: invoke,
    event: {
      listen: function (name, handler) {
        var stop = on(name, function (data) {
          handler({ event: name, payload: data });
        });
        return Promise.resolve(stop);
      }
    },
    install: function () {
      window.__TAURI__ = { core: { invoke: invoke }, event: tauri.event };
      window.__TAURI_INTERNALS__ = { invoke: invoke };
    }
  };

  // Normalisation shim (spec §6.4). Opt-in through `tpt.normalize(options)`; each option does one
  // thing and can be left out: viewport (a viewport meta tag), font (a default font stack on
  // the root), scrollbars (a stable gutter), dragRegion (`data-tpt-drag` areas, with
  // `data-tpt-no-drag` inside them excluded), safeArea (`--tpt-safe-*` custom properties).
  var DEFAULT_FONT = 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif';
  function normalize(options) {
    var opts = options || {};
    var doc = window.document;
    if (!doc) {
      return false;
    }
    var css = '';
    if (opts.scrollbars) {
      css += 'html{scrollbar-gutter:stable}';
    }
    if (opts.dragRegion) {
      css += '[data-tpt-drag]{-webkit-app-region:drag}[data-tpt-no-drag]{-webkit-app-region:no-drag}';
    }
    if (opts.safeArea) {
      css += ':root{--tpt-safe-top:env(safe-area-inset-top,0px);--tpt-safe-right:env(safe-area-inset-right,0px);' +
        '--tpt-safe-bottom:env(safe-area-inset-bottom,0px);--tpt-safe-left:env(safe-area-inset-left,0px)}';
    }
    if (css && doc.head) {
      var style = doc.createElement('style');
      style.setAttribute('data-tpt-normalize', '');
      style.textContent = css;
      doc.head.appendChild(style);
    }
    if (opts.viewport && doc.head && !doc.querySelector('meta[name="viewport"]')) {
      var meta = doc.createElement('meta');
      meta.name = 'viewport';
      meta.content = 'width=device-width, initial-scale=1';
      doc.head.appendChild(meta);
    }
    if (opts.font && doc.documentElement) {
      doc.documentElement.style.fontFamily = opts.font === true ? DEFAULT_FONT : opts.font;
    }
    return true;
  }

  // The APIs this runtime provides. Pages can check `tpt.caps.apis` before they call one.
  var apis = ['invoke', 'on', 'job', 'normalize', 'compat.tauri'];

  // The profile level is set by the host before this file runs (window.tptProfileLevel). Anything
  // else is reported as unknown, so a page never reads a level the host did not give.
  var profileLevels = ['full', 'partial', 'unsupported'];
  var profileLevel = profileLevels.indexOf(window.tptProfileLevel) >= 0 ? window.tptProfileLevel : 'unknown';

  window.tpt = {
    invoke: invoke,
    on: on,
    job: job,
    normalize: normalize,
    compat: { tauri: tauri },
    caps: {
      engine: 'webview2',
      version: edge ? edge[1] : null,
      profile: profileLevel,
      apis: apis.slice()
    }
  };
})();
