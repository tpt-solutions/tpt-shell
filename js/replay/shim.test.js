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
