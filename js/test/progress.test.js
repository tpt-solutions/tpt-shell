// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Tests the optional <tpt-progress> component with a fake element and job handle. Run with:
// node --test js/test/progress.test.js
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const SOURCE = fs.readFileSync(path.join(__dirname, '..', 'components', 'tpt-progress.js'), 'utf8');

class FakeElement {
  constructor() {
    this.attrs = {};
  }
  getAttribute(name) {
    return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
  }
  setAttribute(name, value) {
    this.attrs[name] = String(value);
  }
}

function load() {
  const defined = new Map();
  const window = {
    HTMLElement: FakeElement,
    customElements: {
      get: (name) => defined.get(name),
      define: (name, cls) => defined.set(name, cls),
    },
  };
  vm.runInContext(SOURCE, vm.createContext({ window }));
  return { window, defined };
}

function fakeHandle() {
  const listeners = {};
  return {
    on(name, fn) {
      (listeners[name] = listeners[name] || []).push(fn);
      return () => {
        listeners[name] = (listeners[name] || []).filter((f) => f !== fn);
      };
    },
    emit(name, data) {
      (listeners[name] || []).slice().forEach((fn) => fn(data));
    },
    count(name) {
      return (listeners[name] || []).length;
    },
  };
}

test('the element is defined once, with progressbar semantics', () => {
  const { defined } = load();
  const Cls = defined.get('tpt-progress');
  assert.ok(Cls);
  const el = new Cls();
  el.connectedCallback();
  assert.equal(el.getAttribute('role'), 'progressbar');
  assert.equal(el.getAttribute('aria-valuemax'), '100');
});

test('progress events set the percentage and results finish the element', () => {
  const { defined } = load();
  const el = new (defined.get('tpt-progress'))();
  const handle = fakeHandle();
  el.track(handle);
  handle.emit('job.progress', { job: 1, done: 1, total: 4 });
  assert.equal(el.getAttribute('data-value'), '25');
  assert.equal(el.getAttribute('data-state'), 'running');
  handle.emit('job.result', { job: 1, data: null });
  assert.equal(el.getAttribute('data-state'), 'result');
  assert.equal(el.getAttribute('aria-valuenow'), '100');
  assert.equal(handle.count('job.progress'), 0, 'listeners are released once the job ends');
});

test('a cancelled job is marked cancelled, and a zero total is ignored', () => {
  const { defined } = load();
  const el = new (defined.get('tpt-progress'))();
  const handle = fakeHandle();
  el.track(handle);
  handle.emit('job.progress', { job: 1, done: 0, total: 0 });
  assert.equal(el.getAttribute('data-value'), null);
  handle.emit('job.cancelled', { job: 1 });
  assert.equal(el.getAttribute('data-state'), 'cancelled');
});

test('nothing is defined where customElements is missing', () => {
  const window = {};
  vm.runInContext(SOURCE, vm.createContext({ window }));
  assert.equal(window.TptProgress, undefined);
});
