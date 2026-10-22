// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Accessibility checks for the WCAG 2.2 AA subset (spec §10.6).
//!
//! The rules take values a test has already read from the page: numbers, names, focus stops,
//! a traced Tab sequence and running animations. They do not depend on a browser, so they are
//! unit-tested here. The driver collects the values from the live page (see the test driver
//! in `crate::driver`), and the seeded-violation fixture under `fixtures/a11y-seeded` checks
//! that the rules fire on a known-bad page.
//!
//! Support: Full for the rules, which are heuristics where marked. Partial: the browser side
//! that collects focus stops, Tab traces and animations is routed to the platform agents.

/// An sRGB colour with 8 bits per channel.
///
/// Support: Full.
pub type Rgb = [u8; 3];

/// Relative luminance of a colour, from 0 (black) to 1 (white), as WCAG 2.x defines it.
///
/// Support: Full.
pub fn relative_luminance(c: Rgb) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// Contrast ratio between two colours, from 1 (no contrast) to 21 (black on white).
///
/// Support: Full.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Text is "large" at 24 CSS px, or 18.66 px when bold (WCAG 1.4.3).
///
/// Support: Full.
pub fn is_large_text(size_px: f64, bold: bool) -> bool {
    size_px >= 24.0 || (bold && size_px >= 18.66)
}

/// The minimum contrast for text under WCAG 2.2 AA: 4.5 for normal text, 3 for large text.
///
/// Support: Full.
pub fn min_text_contrast(large: bool) -> f64 {
    if large {
        3.0
    } else {
        4.5
    }
}

/// Smallest side of a pointer target under WCAG 2.2 AA (2.5.8), in CSS px.
///
/// Support: Full.
pub const MIN_TARGET_PX: f64 = 24.0;

/// One problem a rule found.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// The rule that failed, such as `contrast` or `target-size`.
    pub rule: &'static str,
    pub message: String,
}

/// Checks text colour against its background. `large` is from [`is_large_text`].
///
/// Support: Full.
pub fn check_text_contrast(fg: Rgb, bg: Rgb, large: bool) -> Option<Violation> {
    let ratio = contrast_ratio(fg, bg);
    let needed = min_text_contrast(large);
    (ratio < needed).then(|| Violation {
        rule: "contrast",
        message: format!("contrast is {ratio:.2}:1, needs {needed}:1"),
    })
}

/// Checks a pointer target. The spacing exception in 2.5.8 is not modelled, so a small target
/// that is spaced well from its neighbours is still reported here.
///
/// Support: Partial. The spacing exception in WCAG 2.5.8 is not modelled, so a small target that is
/// spaced well from its neighbours is still reported.
pub fn check_target_size(width_px: f64, height_px: f64) -> Option<Violation> {
    (width_px < MIN_TARGET_PX || height_px < MIN_TARGET_PX).then(|| Violation {
        rule: "target-size",
        message: format!(
            "target is {width_px}x{height_px} px, needs at least {MIN_TARGET_PX}x{MIN_TARGET_PX}"
        ),
    })
}

/// A form control as the accessibility tree reports it.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// The role, such as `textbox` or `checkbox`.
    pub role: String,
    /// The accessible name, or nothing when the control has none.
    pub name: Option<String>,
}

/// Returns a violation for each input control that has no accessible name. Buttons and links
/// are not form inputs here; they are covered by [`check_names`].
///
/// Support: Full.
pub fn check_form_labels(controls: &[Control]) -> Vec<Violation> {
    const INPUT_ROLES: [&str; 5] = ["textbox", "checkbox", "radio", "combobox", "searchbox"];
    controls
        .iter()
        .filter(|c| INPUT_ROLES.contains(&c.role.as_str()))
        .filter(|c| c.name.as_deref().is_none_or(|n| n.trim().is_empty()))
        .map(|c| Violation {
            rule: "form-label",
            message: format!("{} has no accessible name", c.role),
        })
        .collect()
}

/// Roles of controls that must have an accessible name (WCAG 4.1.2).
///
/// Support: Full.
pub const INTERACTIVE_ROLES: [&str; 13] = [
    "button",
    "link",
    "checkbox",
    "radio",
    "textbox",
    "searchbox",
    "combobox",
    "menuitem",
    "tab",
    "switch",
    "slider",
    "spinbutton",
    "option",
];

/// A node of the live accessibility tree, as the page or the native tree reports it.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub role: String,
    pub name: Option<String>,
}

/// Returns a violation for each interactive node without a name (WCAG 4.1.2, from the live
/// tree rather than the DOM). Form inputs are covered by [`check_form_labels`]; this covers
/// buttons, links and the rest.
///
/// Support: Full.
pub fn check_names(nodes: &[TreeNode]) -> Vec<Violation> {
    nodes
        .iter()
        .filter(|n| INTERACTIVE_ROLES.contains(&n.role.as_str()))
        .filter(|n| n.name.as_deref().is_none_or(|s| s.trim().is_empty()))
        .map(|n| Violation {
            rule: "name-role",
            message: format!("{} has no accessible name", n.role),
        })
        .collect()
}

/// A focusable element in tab order, with its place on screen.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct FocusStop {
    pub id: String,
    /// The `tabindex` attribute, or 0 when absent.
    pub tab_index: i32,
    /// Top edge in CSS px, relative to the page.
    pub top_px: f64,
}

/// How far a stop may sit above the previous one before it counts as out of reading order.
/// Heuristic: it allows for a line of text in a row that is slightly lower or higher.
///
/// Support: Partial. A heuristic tolerance for the reading-order check. It is not tuned for every
/// layout.
pub const ROW_TOLERANCE_PX: f64 = 8.0;

/// Checks focus order (WCAG 2.4.3). `stops` is the order Tab visits them.
///
/// Reports a positive `tabindex` (it overrides the document order). It also reports a stop
/// that sits clearly above the one before it, which usually means the tab order does not
/// follow the visual order. That check is a heuristic: a deliberate jump can be correct.
///
/// Support: Partial. A heuristic: a deliberate jump in tab order can be correct, and the check
/// still reports it.
pub fn check_focus_order(stops: &[FocusStop]) -> Vec<Violation> {
    let mut found = Vec::new();
    for stop in stops.iter().filter(|s| s.tab_index > 0) {
        found.push(Violation {
            rule: "focus-order",
            message: format!(
                "{} has tabindex {}, which overrides the document order",
                stop.id, stop.tab_index
            ),
        });
    }
    for pair in stops.windows(2) {
        let (prev, next) = (&pair[0], &pair[1]);
        if next.top_px + ROW_TOLERANCE_PX < prev.top_px {
            found.push(Violation {
                rule: "focus-order",
                message: format!(
                    "{} comes after {} in tab order but sits above it on screen",
                    next.id, prev.id
                ),
            });
        }
    }
    found
}

/// Checks a keyboard trap (WCAG 2.1.2). `trace` lists the element focused by each Tab press,
/// starting from a control inside a region. `exits` lists controls outside the region that
/// focus should reach.
///
/// A trap is a trace that returns to an element it already visited before it reaches any exit.
///
/// Support: Full.
pub fn check_keyboard_trap(trace: &[String], exits: &[String]) -> Option<Violation> {
    let mut seen: Vec<&str> = Vec::new();
    for id in trace {
        if exits.contains(id) {
            return None;
        }
        if seen.contains(&id.as_str()) {
            return Some(Violation {
                rule: "keyboard-trap",
                message: format!("focus returns to {id} without reaching an element outside it"),
            });
        }
        seen.push(id);
    }
    None
}

/// A CSS animation or transition on the page, with its computed duration.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub duration_ms: f64,
    /// True when the motion is essential to the task (such as a progress indicator).
    pub essential: bool,
}

/// Durations at or below this are treated as instant (under 5 ms is below one frame).
///
/// Support: Partial. A heuristic threshold: it treats the declared duration as the animation's
/// length.
pub const INSTANT_MS: f64 = 5.0;

/// Checks reduced-motion respect (WCAG 2.3.3, as a heuristic). When the user prefers reduced
/// motion, every non-essential animation must be effectively instant.
///
/// Support: Partial. A heuristic that checks the declared durations of animations, not what the
/// page renders.
pub fn check_reduced_motion(prefers_reduced: bool, animations: &[Animation]) -> Vec<Violation> {
    if !prefers_reduced {
        return Vec::new();
    }
    animations
        .iter()
        .filter(|a| !a.essential && a.duration_ms > INSTANT_MS)
        .map(|a| Violation {
            rule: "reduced-motion",
            message: format!(
                "{} runs for {} ms although the user prefers reduced motion",
                a.name, a.duration_ms
            ),
        })
        .collect()
}

/// Every rule name this crate can report. The seeded-violation fixture lists its expected
/// rules from this set.
///
/// Support: Full.
pub const RULES: [&str; 7] = [
    "contrast",
    "target-size",
    "form-label",
    "name-role",
    "focus-order",
    "keyboard-trap",
    "reduced-motion",
];

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgb = [0, 0, 0];
    const WHITE: Rgb = [255, 255, 255];

    #[test]
    fn black_on_white_is_the_full_ratio() {
        assert!((contrast_ratio(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(WHITE, BLACK) - 21.0).abs() < 1e-9);
    }

    #[test]
    fn a_colour_on_itself_has_no_contrast() {
        assert!((contrast_ratio([10, 20, 30], [10, 20, 30]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn grey_767676_on_white_is_just_over_the_normal_text_minimum() {
        let ratio = contrast_ratio([0x76, 0x76, 0x76], WHITE);
        assert!((ratio - 4.54).abs() < 0.01, "{ratio}");
        assert!(check_text_contrast([0x76, 0x76, 0x76], WHITE, false).is_none());
        assert!(check_text_contrast([0x77, 0x77, 0x77], WHITE, false).is_some());
    }

    #[test]
    fn large_text_needs_only_three_to_one() {
        // About 3.2:1 on white: fails for normal text, passes for large text.
        let grey = [0x94, 0x94, 0x94];
        assert!(check_text_contrast(grey, WHITE, false).is_some());
        assert!(check_text_contrast(grey, WHITE, true).is_none());
    }

    #[test]
    fn large_text_is_24px_or_bold_18_66px() {
        assert!(is_large_text(24.0, false));
        assert!(!is_large_text(23.9, false));
        assert!(is_large_text(18.66, true));
        assert!(!is_large_text(18.66, false));
    }

    #[test]
    fn small_targets_are_reported() {
        assert!(check_target_size(24.0, 24.0).is_none());
        let v = check_target_size(20.0, 30.0).expect("too narrow");
        assert_eq!(v.rule, "target-size");
        assert!(check_target_size(32.0, 16.0).is_some());
    }

    #[test]
    fn unnamed_inputs_are_reported_and_other_roles_are_not() {
        let controls = [
            Control {
                role: "textbox".into(),
                name: Some("Email".into()),
            },
            Control {
                role: "checkbox".into(),
                name: None,
            },
            Control {
                role: "combobox".into(),
                name: Some("   ".into()),
            },
            Control {
                role: "button".into(),
                name: None,
            },
        ];
        let found = check_form_labels(&controls);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|v| v.rule == "form-label"));
    }

    #[test]
    fn interactive_nodes_need_names_in_the_live_tree() {
        let nodes = [
            TreeNode {
                role: "button".into(),
                name: None,
            },
            TreeNode {
                role: "link".into(),
                name: Some("Home".into()),
            },
            TreeNode {
                role: "group".into(),
                name: None,
            },
        ];
        let found = check_names(&nodes);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].rule, "name-role");
    }

    #[test]
    fn positive_tabindex_and_upward_jumps_are_reported() {
        let stop = |id: &str, tab_index: i32, top: f64| FocusStop {
            id: id.into(),
            tab_index,
            top_px: top,
        };
        let clean = [stop("a", 0, 10.0), stop("b", 0, 12.0), stop("c", 0, 60.0)];
        assert!(check_focus_order(&clean).is_empty());
        let bad = [stop("a", 0, 100.0), stop("b", 2, 100.0), stop("c", 0, 20.0)];
        // One for the positive tabindex, one for stop c sitting above stop b.
        assert_eq!(check_focus_order(&bad).len(), 2);
    }

    #[test]
    fn a_trace_that_loops_without_an_exit_is_a_trap() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let exits = ids(&["outside"]);
        let trapped = ids(&["in-1", "in-2", "in-1"]);
        assert_eq!(
            check_keyboard_trap(&trapped, &exits).map(|v| v.rule),
            Some("keyboard-trap")
        );
        let escapes = ids(&["in-1", "in-2", "outside", "in-1"]);
        assert!(check_keyboard_trap(&escapes, &exits).is_none());
    }

    #[test]
    fn animations_must_be_instant_under_reduced_motion() {
        let anims = [
            Animation {
                name: "slide".into(),
                duration_ms: 300.0,
                essential: false,
            },
            Animation {
                name: "spinner".into(),
                duration_ms: 800.0,
                essential: true,
            },
        ];
        assert!(check_reduced_motion(false, &anims).is_empty());
        let found = check_reduced_motion(true, &anims);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.starts_with("slide"));
    }

    #[test]
    fn every_reported_rule_is_registered() {
        for rule in [
            "contrast",
            "target-size",
            "form-label",
            "name-role",
            "focus-order",
            "keyboard-trap",
            "reduced-motion",
        ] {
            assert!(RULES.contains(&rule), "{rule}");
        }
    }

    /// The seeded-violation fixture: every expected violation names a rule this crate can
    /// report, and its element exists on the page. The browser run that checks the page
    /// itself is routed to the platform agents.
    #[test]
    fn the_seeded_fixture_lists_only_known_rules_on_real_elements() {
        const EXPECTED: &str = include_str!("../../../fixtures/a11y-seeded/expected.json");
        const PAGE: &str = include_str!("../../../fixtures/a11y-seeded/index.html");
        let doc: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
        let violations = doc["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 7);
        for v in violations {
            let rule = v["rule"].as_str().unwrap();
            assert!(RULES.contains(&rule), "{rule}");
            let id = v["id"].as_str().unwrap();
            assert!(
                PAGE.contains(&format!("id=\"{id}\"")),
                "{id} is not on the page"
            );
        }
        for id in doc["clean"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            assert!(
                PAGE.contains(&format!("id=\"{id}\"")),
                "{id} is not on the page"
            );
        }
    }
}
