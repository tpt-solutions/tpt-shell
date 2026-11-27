// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runs the accessibility rules against a live page (spec §10.6, todo M8 audit gate).
//!
//! [`COLLECT_SCRIPT`] runs in the page through the test driver's `eval` and returns JSON: one
//! record per element that has an `id`, with its role, name, colours, size, focusability and
//! running animations. [`audit`] turns that JSON into rule inputs and runs the rules in
//! [`crate::a11y`], attributing every violation to an element id. [`trap`] checks a Tab trace the
//! test driver collects by pressing Tab.
//!
//! Limits: only elements with an `id` are collected, so an element without one is not audited.
//! Text colour is taken from the computed style, and the background from the nearest ancestor
//! with a solid background (white if none). Reduced motion is checked against the preference the
//! caller passes, because the page cannot change the system setting it is read from.
//!
//! Support: Partial. The rules are heuristics where marked in [`crate::a11y`]; this module is
//! tested against the seeded fixture on WebView2 and on synthetic JSON.

use serde_json::Value;

use crate::a11y::{
    check_focus_order, check_form_labels, check_names, check_reduced_motion, check_target_size,
    check_text_contrast, is_large_text, Animation, Control, FocusStop, Rgb, TreeNode,
};

/// The script that collects the page's data. It returns a JSON string.
///
/// Support: Partial. Only elements with an `id` are collected.
pub const COLLECT_SCRIPT: &str = r#"(() => {
  const implicitRole = (el) => {
    const explicit = el.getAttribute('role');
    if (explicit) return explicit;
    switch (el.tagName) {
      case 'A': return el.hasAttribute('href') ? 'link' : 'generic';
      case 'BUTTON': return 'button';
      case 'SELECT': return 'combobox';
      case 'TEXTAREA': return 'textbox';
      case 'INPUT': {
        const t = (el.type || 'text').toLowerCase();
        if (t === 'checkbox' || t === 'radio') return t;
        if (t === 'button' || t === 'submit' || t === 'reset') return 'button';
        if (t === 'search') return 'searchbox';
        return 'textbox';
      }
      default: return 'generic';
    }
  };
  const nameOf = (el) => {
    const label = el.getAttribute('aria-label');
    if (label && label.trim()) return label.trim();
    const ids = el.getAttribute('aria-labelledby');
    if (ids) {
      const text = ids.split(/\s+/).map((i) => (document.getElementById(i) || {}).textContent || '').join(' ').trim();
      if (text) return text;
    }
    if (['INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName)) {
      const lab = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
      return lab && lab.textContent.trim() ? lab.textContent.trim() : null;
    }
    const text = (el.textContent || '').trim();
    return text || null;
  };
  const parseRgb = (s) => {
    const m = s.match(/rgba?\(([^)]+)\)/);
    if (!m) return null;
    const p = m[1].split(',').map((x) => parseFloat(x));
    if (p.length === 4 && p[3] === 0) return null;
    return [p[0], p[1], p[2]];
  };
  const bgOf = (el) => {
    for (let n = el; n; n = n.parentElement) {
      const c = parseRgb(getComputedStyle(n).backgroundColor);
      if (c) return c;
    }
    return [255, 255, 255];
  };
  const shown = (el) => el.getClientRects().length > 0;
  const ownText = (el) => Array.from(el.childNodes).some((n) => n.nodeType === 3 && n.textContent.trim());
  const elements = [];
  for (const el of document.querySelectorAll('[id]')) {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    const isShown = shown(el);
    const animations = el.getAnimations ? el.getAnimations().map((a) => ({
      duration_ms: Number(a.effect.getComputedTiming().duration) || 0,
      essential: el.dataset.essential === 'true',
    })) : [];
    elements.push({
      id: el.id,
      tag: el.tagName.toLowerCase(),
      role: implicitRole(el),
      name: nameOf(el),
      text: ownText(el),
      fg: parseRgb(cs.color) || [0, 0, 0],
      bg: bgOf(el),
      font_px: parseFloat(cs.fontSize),
      bold: parseInt(cs.fontWeight, 10) >= 600,
      width: r.width,
      height: r.height,
      shown: isShown,
      focusable: isShown && el.matches('a[href],button,input,select,textarea,[tabindex]') && !el.disabled,
      tab_index: el.tabIndex,
      top: r.top + window.scrollY,
      animations,
    });
  }
  return JSON.stringify({
    prefers_reduced: window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    elements,
  });
})()"#;

/// A violation attributed to an element.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub rule: String,
    pub id: String,
    pub message: String,
}

/// Elements that take a pointer target and count for the target-size rule.
const TARGET_TAGS: [&str; 5] = ["a", "button", "input", "select", "textarea"];

fn text(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("element has no string {key}"))
}

fn number(value: &Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("element has no number {key}"))
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn colour(value: &Value, key: &str) -> Result<Rgb, String> {
    let parts = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("element has no colour {key}"))?;
    if parts.len() != 3 {
        return Err(format!("colour {key} needs three channels"));
    }
    let mut rgb = [0u8; 3];
    for (slot, part) in rgb.iter_mut().zip(parts) {
        let v = part
            .as_f64()
            .ok_or_else(|| format!("colour {key} is not numeric"))?;
        *slot = v.round().clamp(0.0, 255.0) as u8;
    }
    Ok(rgb)
}

/// Runs the rules over the JSON that [`COLLECT_SCRIPT`] returns. `prefers_reduced` says whether
/// the audit assumes the user prefers reduced motion; the page's own preference is reported in
/// the JSON but the caller decides.
///
/// Support: Partial. Heuristic rules, see [`crate::a11y`].
pub fn audit(page: &Value, prefers_reduced: bool) -> Result<Vec<Found>, String> {
    let elements = page
        .get("elements")
        .and_then(Value::as_array)
        .ok_or("the page data has no elements")?;
    let mut found = Vec::new();
    let mut stops = Vec::new();
    let mut animations = Vec::new();

    for el in elements {
        let id = text(el, "id")?;
        let tag = text(el, "tag")?;
        let role = text(el, "role")?;
        let name = el.get("name").and_then(Value::as_str).map(str::to_string);
        let shown = flag(el, "shown");
        if !shown {
            // Hidden elements are not rendered, so no rule can read a colour or a size.
            continue;
        }

        if flag(el, "text") {
            let fg = colour(el, "fg")?;
            let bg = colour(el, "bg")?;
            let large = is_large_text(number(el, "font_px")?, flag(el, "bold"));
            if let Some(v) = check_text_contrast(fg, bg, large) {
                found.push(found_of(v.rule, &id, v.message));
            }
        }
        if TARGET_TAGS.contains(&tag.as_str()) {
            let (w, h) = (number(el, "width")?, number(el, "height")?);
            if let Some(v) = check_target_size(w, h) {
                found.push(found_of(v.rule, &id, v.message));
            }
        }
        if crate::a11y::INTERACTIVE_ROLES.contains(&role.as_str()) {
            let node = TreeNode {
                role: role.clone(),
                name: name.clone(),
            };
            for v in check_names(&[node]) {
                found.push(found_of(v.rule, &id, v.message));
            }
        }
        if matches!(tag.as_str(), "input" | "select" | "textarea") {
            let control = Control {
                role: role.clone(),
                name: name.clone(),
            };
            for v in check_form_labels(&[control]) {
                found.push(found_of(v.rule, &id, v.message));
            }
        }
        if flag(el, "focusable") {
            stops.push(FocusStop {
                id: id.clone(),
                tab_index: el.get("tab_index").and_then(Value::as_i64).unwrap_or(0) as i32,
                top_px: number(el, "top")?,
            });
        }
        if let Some(list) = el.get("animations").and_then(Value::as_array) {
            for anim in list {
                animations.push(Animation {
                    name: id.clone(),
                    duration_ms: number(anim, "duration_ms")?,
                    essential: flag(anim, "essential"),
                });
            }
        }
    }

    // Focus order reports the id as the first word of each message.
    for v in check_focus_order(&stops) {
        let id = v.message.split(' ').next().unwrap_or_default().to_string();
        found.push(found_of(v.rule, &id, v.message));
    }
    for v in check_reduced_motion(prefers_reduced, &animations) {
        let id = v.message.split(' ').next().unwrap_or_default().to_string();
        found.push(found_of(v.rule, &id, v.message));
    }
    Ok(found)
}

/// Checks a keyboard trap from a Tab trace. `trace` starts at the element that had focus when the
/// trace began, then lists the element focused after each Tab press. `exits` lists elements
/// outside the region that focus should reach. The trap is reported against `container`.
///
/// Support: Full.
pub fn trap(container: &str, trace: &[String], exits: &[String]) -> Option<Found> {
    crate::a11y::check_keyboard_trap(trace, exits).map(|v| found_of(v.rule, container, v.message))
}

fn found_of(rule: &str, id: &str, message: String) -> Found {
    Found {
        rule: rule.to_string(),
        id: id.to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn el(id: &str, tag: &str, role: &str, name: Option<&str>) -> Value {
        json!({
            "id": id, "tag": tag, "role": role, "name": name,
            "text": true, "fg": [17, 17, 17], "bg": [255, 255, 255],
            "font_px": 16.0, "bold": false, "width": 80.0, "height": 44.0,
            "shown": true, "focusable": false, "tab_index": 0, "top": 0.0, "animations": []
        })
    }

    #[test]
    fn each_violation_is_attributed_to_its_element() {
        let mut low = el("low", "p", "generic", None);
        low["fg"] = json!([153, 153, 153]);
        let mut nameless = el("nameless", "button", "button", None);
        nameless["width"] = json!(16.0);
        nameless["height"] = json!(16.0);
        let mut unlabelled = el("field", "input", "textbox", None);
        unlabelled["text"] = json!(false);
        let mut late = el("late", "a", "link", Some("Skip"));
        late["focusable"] = json!(true);
        late["tab_index"] = json!(2);
        let mut pulse = el("pulse", "p", "generic", None);
        pulse["animations"] = json!([{ "duration_ms": 600.0, "essential": false }]);
        let page = json!({ "prefers_reduced": false, "elements": [low, nameless, unlabelled, late, pulse] });

        let found = audit(&page, true).expect("audit");
        let has = |rule: &str, id: &str| found.iter().any(|f| f.rule == rule && f.id == id);
        assert!(has("contrast", "low"), "{found:?}");
        assert!(has("name-role", "nameless"), "{found:?}");
        assert!(has("target-size", "nameless"), "{found:?}");
        assert!(has("form-label", "field"), "{found:?}");
        assert!(has("focus-order", "late"), "{found:?}");
        assert!(has("reduced-motion", "pulse"), "{found:?}");
    }

    #[test]
    fn reduced_motion_is_not_reported_without_the_preference() {
        let mut pulse = el("pulse", "p", "generic", None);
        pulse["animations"] = json!([{ "duration_ms": 600.0, "essential": false }]);
        let page = json!({ "elements": [pulse] });
        assert!(audit(&page, false).expect("audit").is_empty());
    }

    #[test]
    fn hidden_elements_are_not_audited() {
        let mut hidden = el("gone", "button", "button", None);
        hidden["shown"] = json!(false);
        let page = json!({ "elements": [hidden] });
        assert!(audit(&page, true).expect("audit").is_empty());
    }

    #[test]
    fn a_trap_is_reported_against_its_container() {
        let trace: Vec<String> = ["a", "b", "a"].iter().map(|s| s.to_string()).collect();
        let found = trap("dialog", &trace, &[]).expect("trap");
        assert_eq!(
            (found.rule.as_str(), found.id.as_str()),
            ("keyboard-trap", "dialog")
        );
        assert!(trap("dialog", &trace, &["a".to_string()]).is_none());
    }

    #[test]
    fn bad_data_is_an_error_not_a_panic() {
        assert!(audit(&json!({}), false).is_err());
        assert!(audit(&json!({ "elements": [{ "id": "x" }] }), false).is_err());
    }
}
