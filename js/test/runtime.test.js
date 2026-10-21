// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Runs js/runtime/tpt-shell.js against a fake chrome.webview. Run with: node --test js/test/
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const SOURCE = fs.readFileSync(path.join(__dirname, '..', 'runtime', 'tpt-shell.js'), 'utf8');

// Builds a page window with a fake WebView2 host. `fromRust(msg)` delivers a message to the page.
function loadRuntime({ userAgent = 'Mozilla/5.0 Chrome/155.0 Safari/537.36 Edg/155.0.4283.62' } = {}) {
  const listeners = [];
  const sent = [];
  const window = {
    chrome: {
      webview: {
        addEventListener(type, fn) {
          if (type === 'message') listeners.push(fn);
        },
        postMessage(text) {
          sent.push(JSON.parse(text));
        },
      },
    },
  };
  const context = vm.createContext({
    window,
    navigator: { userAgent },
    setTimeout: (fn) => fn(),
    Map,
    Promise,
    JSON,
  });
  vm.runInContext(SOURCE, context);
  const fromRust = (data) => listeners.forEach((fn) => fn({ data }));
  return { tpt: window.tpt, sent, fromRust };
}

test('invoke sends a v1 envelope and resolves with the matching response', async () => {
  const { tpt, sent, fromRust } = loadRuntime();
  const call = tpt.invoke('greet', { name: 'x' });
  assert.deepEqual(sent, [{ v: 1, id: 1, cmd: 'greet', args: { name: 'x' } }]);
  fromRust(JSON.stringify({ id: 1, ok: true, data: 'hi' }));
  assert.equal(await call, 'hi');
});

test('invoke rejects with the error object of a failed response', async () => {
  const { tpt, fromRust } = loadRuntime();
  const call = tpt.invoke('missing');
  fromRust({ id: 1, ok: false, error: { code: 'TS0100', message: 'no such command' } });
  await assert.rejects(call, { code: 'TS0100' });
});

test('on receives events by name, with their data', () => {
  const { tpt, fromRust } = loadRuntime();
  const seen = [];
  tpt.on('scan:progress', (data) => seen.push(data));
  fromRust({ event: 'scan:progress', data: { done: 3 } });
  fromRust({ event: 'other', data: 1 });
  assert.deepEqual(seen, [{ done: 3 }]);
});

test('the function returned by on unsubscribes only that handler', () => {
  const { tpt, fromRust } = loadRuntime();
  const first = [];
  const second = [];
  const stopFirst = tpt.on('tick', (n) => first.push(n));
  tpt.on('tick', (n) => second.push(n));
  fromRust({ event: 'tick', data: 1 });
  stopFirst();
  stopFirst();
  fromRust({ event: 'tick', data: 2 });
  assert.deepEqual(first, [1]);
  assert.deepEqual(second, [1, 2]);
});

test('a handler that removes itself during dispatch does not skip the next handler', () => {
  const { tpt, fromRust } = loadRuntime();
  const seen = [];
  const stop = tpt.on('once', () => {
    seen.push('a');
    stop();
  });
  tpt.on('once', () => seen.push('b'));
  fromRust({ event: 'once', data: null });
  assert.deepEqual(seen, ['a', 'b']);
});

test('caps reports the engine and the WebView2 version from the user agent', () => {
  const { tpt } = loadRuntime();
  assert.deepEqual({ ...tpt.caps }, { engine: 'webview2', version: '155.0.4283.62' });
});

test('caps has a null version when the user agent does not name one', () => {
  const { tpt } = loadRuntime({ userAgent: 'Something/1.0' });
  assert.deepEqual({ ...tpt.caps }, { engine: 'webview2', version: null });
});

test('the runtime does nothing without chrome.webview', () => {
  const context = vm.createContext({ window: {}, navigator: { userAgent: '' } });
  vm.runInContext(SOURCE, context);
  assert.equal(context.window.tpt, undefined);
});
