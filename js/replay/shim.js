// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// In-page shim for record and replay (spec 10.5). It virtualises Date.now, performance.now,
// Math.random and timers. It is installed only in record or replay runs, never in a normal
// run, and the host calls it only when the replay feature is on.
//
// The virtual clock moves only when the host calls advance(ms). In a recording the host calls
// advance with the real elapsed time and records each amount (onEvent receives it as a clock
// event). In a replay the host calls advance with the recorded amounts, in the same order, so
// timers fire at the same virtual times. Math.random is a seeded generator, so a replay with
// the same seed draws the same numbers. Timers run in order of due time, then of creation.
//
// Limits: `new Date()` is not virtualised (only Date.now is), and promises and
// requestAnimationFrame are not virtualised. Both are documented in docs/replay-format.md.
//
// Usage:
//   const shim = TptReplayShim.install({ seed: 42, start: 0, onEvent: (kind, data) => ... });
//   shim.advance(16);
//   shim.restore();

(function (root) {
  'use strict';

  const MAX_FIRINGS_PER_ADVANCE = 10000;
  // Asset names as the Rust reader accepts them (crates/tpt-shell-replay is_asset_name).
  const ASSET_NAME = /^[A-Za-z0-9._-]{1,100}$/;
  const NAME_LIMIT = 80;
  const IMPLICIT_ROLE = {
    A: 'link', BUTTON: 'button', SELECT: 'combobox', TEXTAREA: 'textbox', IMG: 'img',
    H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading',
  };

  function install(options) {
    const opts = Object.assign(
      { seed: 1, start: 0, onEvent: null, recordDom: false, windowName: 'main' },
      options || {}
    );
    if (!Number.isFinite(opts.start) || opts.start < 0) {
      throw new RangeError('start must be a non-negative number');
    }
    let state = (opts.seed >>> 0) || 1;
    let nowMs = Math.floor(opts.start);
    const startMs = nowMs;
    let nextId = 1;
    const timers = new Map();

    const emit = (kind, data) => {
      if (typeof opts.onEvent === 'function') opts.onEvent(kind, data);
    };

    // mulberry32: a small generator with a fixed sequence for a fixed seed.
    const draw = () => {
      state = (state + 0x6d2b79f5) >>> 0;
      let t = state;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };

    const originals = {
      dateNow: Date.now,
      perfNow: root.performance ? root.performance.now : undefined,
      random: Math.random,
      setTimeout: root.setTimeout,
      clearTimeout: root.clearTimeout,
      setInterval: root.setInterval,
      clearInterval: root.clearInterval,
    };

    const addTimer = (fn, ms, args, repeat) => {
      if (typeof fn !== 'function') throw new TypeError('the callback must be a function');
      const delay = Math.max(repeat ? 1 : 0, Math.floor(Number(ms) || 0));
      const id = nextId++;
      timers.set(id, { due: nowMs + delay, delay, fn, args, repeat, order: id });
      return id;
    };

    const nextTimer = (limit) => {
      let best = null;
      for (const [id, t] of timers) {
        if (t.due > limit) continue;
        if (best === null || t.due < best.t.due || (t.due === best.t.due && t.order < best.t.order)) {
          best = { id, t };
        }
      }
      return best;
    };

    // DOM capture. A click, key or input is recorded with a stable element path: each step is the
    // element's position among its siblings, its role and its accessible name, from the root down.
    // No CSS selector or id is used, since those break when the page changes. An input's value is
    // never recorded, only its length, and a key typed into a password field is recorded as
    // [redacted].
    const attr = (el, name) => {
      const value = typeof el.getAttribute === 'function' ? el.getAttribute(name) : null;
      return value || '';
    };
    const roleOf = (el) => {
      const explicit = attr(el, 'role');
      if (explicit) return explicit;
      const tag = el.tagName.toUpperCase();
      if (tag === 'INPUT') {
        const type = String(el.type || 'text').toLowerCase();
        if (type === 'button' || type === 'submit' || type === 'reset') return 'button';
        if (type === 'checkbox' || type === 'radio') return type;
        return 'textbox';
      }
      return IMPLICIT_ROLE[tag] || el.tagName.toLowerCase();
    };
    const nameOf = (el) => {
      const text = attr(el, 'aria-label') || attr(el, 'alt') || attr(el, 'title') || el.textContent || '';
      return Array.from(String(text).replace(/\s+/g, ' ').trim()).slice(0, NAME_LIMIT).join('');
    };
    const pathOf = (el) => {
      const steps = [];
      for (let node = el; node && node.tagName; node = node.parentElement) {
        const siblings = node.parentElement ? node.parentElement.children : [node];
        const index = Array.prototype.indexOf.call(siblings, node);
        steps.unshift({ index, role: roleOf(node), name: nameOf(node) });
      }
      return steps;
    };
    const isSecret = (el) => String(el.type || '').toLowerCase() === 'password';
    const listeners = [];
    const listen = (target, type, handler) => {
      if (!target || typeof target.addEventListener !== 'function') return;
      target.addEventListener(type, handler, true);
      listeners.push({ target, type, handler });
    };
    if (opts.recordDom) {
      const doc = root.document;
      listen(doc, 'click', (e) => {
        const x = Math.round(e.clientX || 0);
        const y = Math.round(e.clientY || 0);
        emit('input', { kind: 'click', path: pathOf(e.target), x, y });
      });
      listen(doc, 'keydown', (e) => {
        const key = isSecret(e.target) ? '[redacted]' : String(e.key);
        emit('input', { kind: 'key', path: pathOf(e.target), key });
      });
      listen(doc, 'input', (e) => {
        const value_length = String(e.target.value || '').length;
        emit('input', { kind: 'input', path: pathOf(e.target), value_length });
      });
      listen(root, 'resize', () => {
        emit('window', { op: 'resize', window: opts.windowName, width: root.innerWidth | 0, height: root.innerHeight | 0 });
      });
      listen(root, 'focus', () => emit('window', { op: 'focus', window: opts.windowName }));
      listen(root, 'pagehide', () => emit('window', { op: 'close', window: opts.windowName }));
    }

    const shim = {
      now() {
        return nowMs;
      },

      // Records that the page loaded an app asset, with its HTTP status. The body is not recorded.
      protocolLoad(asset, status) {
        if (typeof asset !== 'string' || !ASSET_NAME.test(asset) || asset === '.' || asset === '..') {
          throw new TypeError('not a valid asset name');
        }
        if (!Number.isInteger(status) || status < 100 || status > 599) {
          throw new RangeError('status must be an HTTP status code');
        }
        emit('protocol_load', { asset, status });
      },

      // Moves the virtual clock forward, firing timers that come due on the way.
      advance(ms) {
        if (!Number.isFinite(ms) || ms < 0) throw new RangeError('advance needs a non-negative number of ms');
        const amount = Math.floor(ms);
        emit('clock', { op: 'advance', ms: amount });
        const target = nowMs + amount;
        let firings = 0;
        for (let hit = nextTimer(target); hit !== null; hit = nextTimer(target)) {
          if (++firings > MAX_FIRINGS_PER_ADVANCE) {
            throw new Error('too many timer firings in one advance; a timer may be looping');
          }
          const { id, t } = hit;
          nowMs = t.due;
          if (t.repeat) {
            t.due += t.delay;
            t.order = nextId++;
          } else {
            timers.delete(id);
          }
          t.fn(...t.args);
        }
        nowMs = target;
      },

      restore() {
        for (const { target, type, handler } of listeners.splice(0)) {
          if (typeof target.removeEventListener === 'function') target.removeEventListener(type, handler, true);
        }
        Date.now = originals.dateNow;
        if (root.performance && originals.perfNow) root.performance.now = originals.perfNow;
        Math.random = originals.random;
        root.setTimeout = originals.setTimeout;
        root.clearTimeout = originals.clearTimeout;
        root.setInterval = originals.setInterval;
        root.clearInterval = originals.clearInterval;
        timers.clear();
      },
    };

    Date.now = () => nowMs;
    if (root.performance) root.performance.now = () => nowMs - startMs;
    Math.random = () => {
      const value = draw();
      emit('rng', { op: 'js-random', value: Math.floor(value * 4294967296) });
      return value;
    };
    root.setTimeout = (fn, ms, ...args) => addTimer(fn, ms, args, false);
    root.setInterval = (fn, ms, ...args) => addTimer(fn, ms, args, true);
    root.clearTimeout = (id) => timers.delete(id);
    root.clearInterval = (id) => timers.delete(id);

    return shim;
  }

  const api = { install };
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.TptReplayShim = api;
  }
})(typeof globalThis !== 'undefined' ? globalThis : this);
