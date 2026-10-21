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
  return { tpt: window.tpt, sent, fromRust, window };
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
  assert.equal(tpt.caps.engine, 'webview2');
  assert.equal(tpt.caps.version, '155.0.4283.62');
});

test('caps has a null version when the user agent does not name one', () => {
  const { tpt } = loadRuntime({ userAgent: 'Something/1.0' });
  assert.equal(tpt.caps.version, null);
});

test('the runtime does nothing without chrome.webview', () => {
  const context = vm.createContext({ window: {}, navigator: { userAgent: '' } });
  vm.runInContext(SOURCE, context);
  assert.equal(context.window.tpt, undefined);
});

test('caps lists the APIs the runtime provides', () => {
  const { tpt } = loadRuntime();
  assert.deepEqual([...tpt.caps.apis], ['invoke', 'on', 'job', 'normalize', 'compat.tauri']);
});

test('job.cancel sends the job.cancel command for that job', async () => {
  const { tpt, sent, fromRust } = loadRuntime();
  const handle = tpt.job(5);
  assert.equal(handle.id, 5);
  const stopping = handle.cancel();
  assert.deepEqual(sent, [{ v: 1, id: 1, cmd: 'job.cancel', args: { job: 5 } }]);
  fromRust({ id: 1, ok: true, data: { cancelled: true } });
  assert.deepEqual(await stopping, { cancelled: true });
});

test('a job handle hears only the events of its own job', () => {
  const { tpt, fromRust } = loadRuntime();
  const mine = [];
  tpt.job(2).on('job.progress', (data) => mine.push(data.done));
  fromRust({ event: 'job.progress', data: { job: 1, done: 9, total: 10 } });
  fromRust({ event: 'job.progress', data: { job: 2, done: 4, total: 10 } });
  assert.deepEqual(mine, [4]);
});

test('the tauri shim maps invoke and event.listen onto the runtime, and installs only on request', async () => {
  const { tpt, sent, fromRust, window } = loadRuntime();
  assert.equal(window.__TAURI__, undefined);
  const call = tpt.compat.tauri.invoke('greet', { name: 'a' });
  assert.deepEqual(sent[0], { v: 1, id: 1, cmd: 'greet', args: { name: 'a' } });
  fromRust({ id: 1, ok: true, data: 'hi' });
  assert.equal(await call, 'hi');

  const got = [];
  const stop = await tpt.compat.tauri.event.listen('tick', (e) => got.push(e));
  fromRust({ event: 'tick', data: 3 });
  stop();
  fromRust({ event: 'tick', data: 4 });
  assert.deepEqual(JSON.parse(JSON.stringify(got)), [{ event: 'tick', payload: 3 }]);

  tpt.compat.tauri.install();
  assert.equal(typeof window.__TAURI__.core.invoke, 'function');
  assert.equal(typeof window.__TAURI_INTERNALS__.invoke, 'function');
});

function fakeDocument() {
  const head = { children: [], appendChild(node) { this.children.push(node); } };
  const created = [];
  return {
    head,
    created,
    documentElement: { style: {} },
    querySelector: () => null,
    createElement(tag) {
      const node = { tag, attrs: {}, setAttribute(k, v) { this.attrs[k] = v; } };
      created.push(node);
      return node;
    },
  };
}

test('normalize does nothing until it is asked, then applies each option it is given', () => {
  const { tpt, window } = loadRuntime();
  window.document = fakeDocument();
  assert.equal(window.document.created.length, 0);

  assert.equal(tpt.normalize({ font: true, dragRegion: true, viewport: true }), true);
  const [style, meta] = window.document.created;
  assert.equal(style.tag, 'style');
  assert.match(style.textContent, /-webkit-app-region:drag/);
  assert.equal(meta.name, 'viewport');
  assert.equal(window.document.documentElement.style.fontFamily.includes('system-ui'), true);
});

test('normalize without a document reports that it did nothing', () => {
  const { tpt } = loadRuntime();
  assert.equal(tpt.normalize({ viewport: true }), false);
});
