// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// tpt-shell agent page helpers (spec §10.4). The shell runs this inside a page, followed by one
// call: `window.__tptAgent.snapshot()` for the accessible tree, or `window.__tptAgent.act(...)`
// for one action on an element of the latest snapshot. It only reads and changes the page the
// user is on; it cannot run anything the page did not already run.
//
// Redaction: a password field's value is never reported, and an element marked
// `data-tpt-redact` is reported as "[redacted]" with its contents left out.
(function () {
  var MAX_NODES = 400;
  var MAX_DEPTH = 24;
  var NAME_LIMIT = 120;
  var READ_LIMIT = 2000;
  var REDACTED = '[redacted]';

  var IMPLICIT = {
    a: 'link', button: 'button', select: 'combobox', textarea: 'textbox', img: 'img',
    nav: 'navigation', main: 'main', header: 'banner', footer: 'contentinfo',
    form: 'form', article: 'article', dialog: 'dialog', ul: 'list', ol: 'list',
    li: 'listitem', table: 'table', h1: 'heading', h2: 'heading', h3: 'heading',
    h4: 'heading', h5: 'heading', h6: 'heading'
  };
  var INPUT_ROLE = {
    button: 'button', submit: 'button', reset: 'button', image: 'button',
    checkbox: 'checkbox', radio: 'radio', range: 'slider', hidden: null
  };

  function clip(text, limit) {
    var s = String(text || '').replace(/\s+/g, ' ').trim();
    return s.length > limit ? s.slice(0, limit) + '...' : s;
  }

  function roleOf(el) {
    var explicit = (el.getAttribute('role') || '').trim().split(/\s+/)[0];
    if (explicit) return explicit;
    var tag = el.tagName.toLowerCase();
    if (tag === 'a') return el.hasAttribute('href') ? 'link' : null;
    if (tag === 'input') {
      var type = (el.getAttribute('type') || 'text').toLowerCase();
      return Object.prototype.hasOwnProperty.call(INPUT_ROLE, type) ? INPUT_ROLE[type] : 'textbox';
    }
    return IMPLICIT[tag] || null;
  }

  function isHidden(el) {
    if (el.hidden || el.getAttribute('aria-hidden') === 'true') return true;
    var style = window.getComputedStyle(el);
    return style.display === 'none' || style.visibility === 'hidden';
  }

  function isPassword(el) {
    return el.tagName === 'INPUT' && (el.getAttribute('type') || '').toLowerCase() === 'password';
  }

  function isSecret(el) {
    return el.hasAttribute('data-tpt-redact');
  }

  // Roles whose name comes from their text. Other roles are named by a label or an attribute only,
  // so a container does not pick up the text of everything inside it.
  var NAME_FROM_CONTENT = {
    button: 1, link: 1, heading: 1, checkbox: 1, radio: 1, menuitem: 1, option: 1, tab: 1,
    switch: 1, listitem: 1, cell: 1, columnheader: 1, rowheader: 1, treeitem: 1
  };

  // The text inside `el`, leaving out redacted elements and hidden ones.
  function textOf(el) {
    var parts = [];
    (function walk(node) {
      Array.prototype.forEach.call(node.childNodes, function (child) {
        if (child.nodeType === 3) parts.push(child.nodeValue);
        else if (child.nodeType === 1 && !isSecret(child) && !isHidden(child)) walk(child);
      });
    })(el);
    return parts.join(' ');
  }

  function nameOf(el, role) {
    var label = el.getAttribute('aria-label');
    if (label) return clip(label, NAME_LIMIT);
    var by = el.getAttribute('aria-labelledby');
    if (by) {
      var joined = by.split(/\s+/).map(function (id) {
        var node = document.getElementById(id);
        return node ? node.textContent : '';
      }).join(' ');
      if (clip(joined, NAME_LIMIT)) return clip(joined, NAME_LIMIT);
    }
    if (el.labels && el.labels.length) {
      var fromLabels = Array.prototype.map.call(el.labels, function (l) {
        return l.textContent;
      }).join(' ');
      if (clip(fromLabels, NAME_LIMIT)) return clip(fromLabels, NAME_LIMIT);
    }
    if (el.tagName === 'IMG') return clip(el.getAttribute('alt'), NAME_LIMIT);
    if (el.tagName === 'INPUT' && el.placeholder) return clip(el.placeholder, NAME_LIMIT);
    if (el.title) return clip(el.title, NAME_LIMIT);
    if (NAME_FROM_CONTENT[role]) return clip(textOf(el), NAME_LIMIT);
    return '';
  }

  function statesOf(el) {
    var states = {};
    if (el.disabled || el.getAttribute('aria-disabled') === 'true') states.disabled = true;
    var isToggle = el.type === 'checkbox' || el.type === 'radio';
    if (isToggle || el.hasAttribute('aria-checked')) {
      states.checked = isToggle ? !!el.checked : el.getAttribute('aria-checked') === 'true';
    }
    var expanded = el.getAttribute('aria-expanded');
    if (expanded !== null) states.expanded = expanded === 'true';
    if (el.getAttribute('aria-selected') === 'true') states.selected = true;
    if (document.activeElement === el) states.focused = true;
    return states;
  }

  function valueOf(el) {
    if (isSecret(el)) return undefined;
    if (isPassword(el)) return REDACTED;
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') {
      return el.value;
    }
    return undefined;
  }

  function boundsOf(el) {
    var r = el.getBoundingClientRect();
    return { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) };
  }

  // Walks `el` and returns the nodes it contributes. An element without a role adds only its
  // children. Each node that gets a reference is pushed onto `refs`, which the action calls use.
  function collect(el, depth, budget, refs) {
    if (el.nodeType !== 1 || budget.left <= 0 || depth > MAX_DEPTH || isHidden(el)) return [];
    var role = roleOf(el);
    var secret = isSecret(el);
    var children = [];
    if (!secret) {
      Array.prototype.forEach.call(el.children, function (child) {
        children = children.concat(collect(child, depth + 1, budget, refs));
      });
    }
    if (!role) return children;
    budget.left -= 1;
    refs.push(el);
    var node = {
      ref: refs.length,
      role: role,
      name: secret ? REDACTED : nameOf(el, role),
      states: statesOf(el),
      bounds: boundsOf(el),
      children: children
    };
    var value = valueOf(el);
    if (value !== undefined) node.value = value;
    return [node];
  }

  function snapshot() {
    var refs = [];
    var budget = { left: MAX_NODES };
    var root = document.body || document.documentElement;
    var tree = collect(root, 0, budget, refs);
    window.__tptAgentRefs = refs;
    return {
      title: clip(document.title, NAME_LIMIT),
      path: location.origin + location.pathname,
      truncated: budget.left <= 0,
      tree: tree
    };
  }

  function target(ref) {
    var refs = window.__tptAgentRefs || [];
    var el = refs[ref - 1];
    if (!el || !el.isConnected) {
      throw new Error('stale reference ' + ref + ': take a new snapshot');
    }
    return el;
  }

  function setNativeValue(el, value) {
    var proto = el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }

  function perform(action) {
    switch (action.op) {
      case 'click': {
        var clicked = target(action.ref);
        if (clicked.disabled) throw new Error('the element is disabled');
        clicked.focus();
        clicked.click();
        return null;
      }
      case 'focus':
        target(action.ref).focus();
        return null;
      case 'type': {
        var typed = target(action.ref);
        if (typed.disabled || typed.readOnly) throw new Error('the field cannot be typed into');
        if (!('value' in typed)) throw new Error('the element is not a text field');
        typed.focus();
        setNativeValue(typed, String(action.text));
        return null;
      }
      case 'select': {
        var picked = target(action.ref);
        if (picked.tagName !== 'SELECT') throw new Error('the element is not a select');
        var has = Array.prototype.some.call(picked.options, function (o) { return o.value === action.value; });
        if (!has) throw new Error('no option with that value');
        picked.value = action.value;
        picked.dispatchEvent(new Event('change', { bubbles: true }));
        return null;
      }
      case 'key': {
        var keyTarget = action.ref ? target(action.ref) : (document.activeElement || document.body);
        keyTarget.dispatchEvent(new KeyboardEvent('keydown', { key: String(action.key), bubbles: true }));
        keyTarget.dispatchEvent(new KeyboardEvent('keyup', { key: String(action.key), bubbles: true }));
        return null;
      }
      case 'scroll': {
        var dy = Number(action.dy) || 0;
        if (action.ref) target(action.ref).scrollBy(0, dy);
        else window.scrollBy(0, dy);
        return null;
      }
      case 'read': {
        var read = target(action.ref);
        if (isSecret(read) || isPassword(read)) throw new Error('refused: this field is redacted');
        if ('value' in read) return clip(read.value, READ_LIMIT);
        return clip(textOf(read), READ_LIMIT);
      }
      default:
        throw new Error('unknown action ' + action.op);
    }
  }

  function act(action) {
    try {
      return { ok: true, data: perform(action) };
    } catch (e) {
      return { ok: false, error: String((e && e.message) || e) };
    }
  }

  window.__tptAgent = { snapshot: snapshot, act: act };
})();
