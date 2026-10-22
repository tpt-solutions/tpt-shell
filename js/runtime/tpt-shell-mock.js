// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// A stand-in for window.tpt when a page is opened in a plain browser tab by `tpt-shell dev`.
// It does nothing inside a tpt-shell window, where the real runtime is already there.
// Plain JavaScript, no build step.
//
// The page can answer commands itself:
//   window.tpt.mock.respond('greet', function (args) { return 'hello ' + args.name; });
// A command with no answer rejects with a MOCK_NO_RESPONDER error that says what to do.
(function () {
  if (window.tpt || (window.chrome && window.chrome.webview)) {
    return;
  }

  var responders = new Map();
  var listeners = new Map();

  function invoke(cmd, args) {
    var respond = responders.get(cmd);
    return new Promise(function (resolve, reject) {
      if (!respond) {
        reject({
          code: 'MOCK_NO_RESPONDER',
          message:
            'no mock for ' + cmd + '. Open the page in a tpt-shell window, or call ' +
            'window.tpt.mock.respond(' + JSON.stringify(cmd) + ', handler)'
        });
        return;
      }
      Promise.resolve()
        .then(function () {
          return respond(args || {});
        })
        .then(resolve, reject);
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

  window.tpt = {
    invoke: invoke,
    on: on,
    caps: { engine: 'mock', version: null },
    mock: {
      respond: function (cmd, handler) {
        responders.set(cmd, handler);
      },
      // Sends an event to the page's `on` handlers, as the shell would.
      emit: function (name, data) {
        (listeners.get(name) || []).slice().forEach(function (handler) {
          handler(data);
        });
      }
    }
  };
  console.info('tpt-shell: window.tpt is a mock (this page is not in a tpt-shell window)');
})();
