// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Tests for js/replay/shim.js. Run with: node --test js/replay/

'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { install } = require('./shim.js');

// Runs a small scripted page with the shim and returns what it observed. The host drives the
// clock with `steps`, and every virtual event goes into `trace`.
function runPage(steps, seed, start) {
  const trace = [];
  const shim = install({
    seed,
    start,
    onEvent: (kind, data) => trace.push({ kind, data }),
  });
  try {
    const seen = [];
    let ticks = 0;
    setTimeout(() => seen.push(['timeout', Date.now()]), 5);
    const interval = setInterval(() => {
      ticks += 1;
      seen.push(['tick', ticks, performance.now()]);
      if (ticks === 3) clearInterval(interval);
    }, 4);
    const randoms = [Math.random(), Math.random()];
    for (const ms of steps) shim.advance(ms);
    return { seen, randoms, trace, now: shim.now() };
  } finally {
    shim.restore();
  }
}

test('timers fire in virtual time, in due order, and intervals repeat until cleared', () => {
  const { seen } = runPage([4, 4, 4], 1, 1000);
  assert.deepEqual(seen, [
    ['tick', 1, 4],
    ['timeout', 1005],
    ['tick', 2, 8],
    ['tick', 3, 12],
  ]);
});

test('Date.now and performance.now follow the virtual clock, not the wall clock', () => {
  const trace = [];
  const shim = install({ start: 500, onEvent: (k, d) => trace.push([k, d]) });
  try {
    assert.equal(Date.now(), 500);
    shim.advance(25);
    assert.equal(Date.now(), 525);
    assert.equal(performance.now(), 25);
  } finally {
    shim.restore();
  }
});

test('the same seed gives the same random numbers, a different seed does not', () => {
  const a = runPage([], 42, 0).randoms;
  const b = runPage([], 42, 0).randoms;
  const c = runPage([], 43, 0).randoms;
  assert.deepEqual(a, b);
  assert.notDeepEqual(a, c);
  for (const v of a) {
    assert.ok(v >= 0 && v < 1);
  }
});

test('a recorded run replays with the same trace and the same timer order', () => {
  const recorded = runPage([3, 7, 2, 9], 7, 0);
  // The replay feeds the recorded advances back in the same order.
  const advances = recorded.trace.filter((e) => e.kind === 'clock').map((e) => e.data.ms);
  const replayed = runPage(advances, 7, 0);
  assert.deepEqual(replayed.trace, recorded.trace);
  assert.deepEqual(replayed.seen, recorded.seen);
  assert.equal(replayed.now, recorded.now);
});

test('the shim records random draws and clock advances, and nothing else', () => {
  const { trace } = runPage([10], 3, 0);
  const kinds = new Set(trace.map((e) => e.kind));
  assert.deepEqual([...kinds].sort(), ['clock', 'rng']);
  assert.equal(trace.filter((e) => e.kind === 'rng').length, 2);
});

test('negative, NaN and infinite advances are refused', () => {
  const shim = install({});
  try {
    assert.throws(() => shim.advance(-1), RangeError);
    assert.throws(() => shim.advance(Number.NaN), RangeError);
    assert.throws(() => shim.advance(Number.POSITIVE_INFINITY), RangeError);
    assert.equal(shim.now(), 0);
  } finally {
    shim.restore();
  }
});

test('restore puts the real globals back', () => {
  const realNow = Date.now;
  const realRandom = Math.random;
  const realSetTimeout = globalThis.setTimeout;
  const shim = install({});
  assert.notEqual(Date.now, realNow);
  shim.restore();
  assert.equal(Date.now, realNow);
  assert.equal(Math.random, realRandom);
  assert.equal(globalThis.setTimeout, realSetTimeout);
});

test('a timer that keeps itself due forever is stopped, not allowed to hang', () => {
  const shim = install({});
  try {
    setInterval(() => {}, 1);
    assert.throws(() => shim.advance(1e9), /too many timer firings/);
  } finally {
    shim.restore();
  }
});

// A fake page for the DOM tests: a target with listeners that a test can fire by hand, and a
// small element tree with the fields the shim reads (tagName, children, parentElement, getAttribute).
function fakeTarget() {
  const handlers = new Map();
  return {
    addEventListener(type, fn) {
      if (!handlers.has(type)) handlers.set(type, []);
      handlers.get(type).push(fn);
    },
    removeEventListener(type, fn) {
      const list = handlers.get(type) || [];
      const at = list.indexOf(fn);
      if (at >= 0) list.splice(at, 1);
    },
    fire(type, event = {}) {
      for (const fn of [...(handlers.get(type) || [])]) fn(event);
    },
  };
}

function element(tag, { role, name, type, value, text, children = [] } = {}) {
  const attrs = { role, 'aria-label': name };
  const el = {
    tagName: tag.toUpperCase(),
    type,
    value,
    textContent: text || '',
    children: [],
    parentElement: null,
    getAttribute: (n) => (attrs[n] === undefined ? null : attrs[n]),
  };
  for (const child of children) {
    child.parentElement = el;
    el.children.push(child);
  }
  return el;
}

// Installs a fake window and document for the duration of `body`, then puts the globals back.
function withFakePage(body) {
  const win = fakeTarget();
  const doc = fakeTarget();
  const saved = {
    document: globalThis.document,
    addEventListener: globalThis.addEventListener,
    removeEventListener: globalThis.removeEventListener,
    innerWidth: globalThis.innerWidth,
    innerHeight: globalThis.innerHeight,
  };
  Object.assign(globalThis, {
    document: doc,
    addEventListener: win.addEventListener,
    removeEventListener: win.removeEventListener,
    innerWidth: 800,
    innerHeight: 600,
  });
  try {
    return body({ win, doc });
  } finally {
    for (const key of Object.keys(saved)) {
      if (saved[key] === undefined) delete globalThis[key];
      else globalThis[key] = saved[key];
    }
  }
}

test('a click records a stable element path of index, role and name, and no selector', () => {
  withFakePage(({ doc }) => {
    const cancel = element('button', { text: 'Cancel' });
    const ok = element('button', { text: 'OK' });
    const main = element('main', { children: [ok, cancel] });
    const body = element('body', { children: [main] });
    element('html', { children: [body] });
    const trace = [];
    const shim = install({ recordDom: true, onEvent: (kind, data) => trace.push({ kind, data }) });
    try {
      doc.fire('click', { target: cancel, clientX: 10.4, clientY: 20.6 });
    } finally {
      shim.restore();
    }
    assert.equal(trace.length, 1);
    assert.equal(trace[0].kind, 'input');
    assert.deepEqual(trace[0].data, {
      kind: 'click',
      path: [
        { index: 0, role: 'html', name: '' },
        { index: 0, role: 'body', name: '' },
        { index: 0, role: 'main', name: '' },
        { index: 1, role: 'button', name: 'Cancel' },
      ],
      x: 10,
      y: 21,
    });
  });
});

test('a key records its name, and a key typed into a password field is redacted', () => {
  withFakePage(({ doc }) => {
    const pin = element('input', { type: 'password', name: 'PIN' });
    const search = element('input', { type: 'text', name: 'Search' });
    element('body', { children: [pin, search] });
    const trace = [];
    const shim = install({ recordDom: true, onEvent: (kind, data) => trace.push(data) });
    try {
      doc.fire('keydown', { target: pin, key: '4' });
      doc.fire('keydown', { target: search, key: 'Enter' });
    } finally {
      shim.restore();
    }
    assert.equal(trace[0].kind, 'key');
    assert.equal(trace[0].key, '[redacted]');
    assert.equal(trace[1].key, 'Enter');
    assert.equal(trace[1].path[trace[1].path.length - 1].role, 'textbox');
  });
});

test('an input records its length and never its value', () => {
  withFakePage(({ doc }) => {
    const field = element('input', { type: 'text', name: 'Card', value: 'secret123' });
    element('body', { children: [field] });
    const trace = [];
    const shim = install({ recordDom: true, onEvent: (kind, data) => trace.push(data) });
    try {
      doc.fire('input', { target: field });
    } finally {
      shim.restore();
    }
    assert.equal(trace[0].value_length, 9);
    assert.equal('value' in trace[0], false);
    assert.equal(JSON.stringify(trace).includes('secret123'), false);
  });
});

test('resize, focus and close record the window name and its size', () => {
  withFakePage(({ win }) => {
    const trace = [];
    const shim = install({
      recordDom: true,
      windowName: 'till',
      onEvent: (kind, data) => trace.push({ kind, data }),
    });
    try {
      win.fire('resize');
      win.fire('focus');
      win.fire('pagehide');
    } finally {
      shim.restore();
    }
    assert.deepEqual(trace, [
      { kind: 'window', data: { op: 'resize', window: 'till', width: 800, height: 600 } },
      { kind: 'window', data: { op: 'focus', window: 'till' } },
      { kind: 'window', data: { op: 'close', window: 'till' } },
    ]);
  });
});

test('a protocol load records the asset and its status, never a body, and refuses bad names', () => {
  const trace = [];
  const shim = install({ onEvent: (kind, data) => trace.push({ kind, data }) });
  try {
    shim.protocolLoad('index.html', 200);
    assert.throws(() => shim.protocolLoad('..', 200), TypeError);
    assert.throws(() => shim.protocolLoad('.', 200), TypeError);
    assert.throws(() => shim.protocolLoad('a/b.js', 200), TypeError);
    assert.throws(() => shim.protocolLoad('x.js', 99), RangeError);
  } finally {
    shim.restore();
  }
  assert.deepEqual(trace, [{ kind: 'protocol_load', data: { asset: 'index.html', status: 200 } }]);
});

test('without recordDom, page events are not recorded', () => {
  withFakePage(({ doc, win }) => {
    const field = element('input', { type: 'text', value: 'abc' });
    element('body', { children: [field] });
    const trace = [];
    const shim = install({ onEvent: (kind, data) => trace.push({ kind, data }) });
    try {
      doc.fire('click', { target: field });
      doc.fire('input', { target: field });
      win.fire('resize');
    } finally {
      shim.restore();
    }
    assert.deepEqual(trace, []);
  });
});

test('restore removes the DOM listeners', () => {
  withFakePage(({ doc }) => {
    const field = element('button', { text: 'Go' });
    element('body', { children: [field] });
    const trace = [];
    const shim = install({ recordDom: true, onEvent: (kind, data) => trace.push(data) });
    shim.restore();
    doc.fire('click', { target: field });
    assert.deepEqual(trace, []);
  });
});
