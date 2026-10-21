// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Accessibility checks for the WCAG 2.2 AA subset (spec §10.6).
//!
//! These are the rules that need only numbers and names: colour contrast, target size, and
//! control names. They run on values a test has already read from the page, so they do not
//! depend on a browser. The rules that need the live tree (focus order, keyboard traps, and
//! reduced-motion behaviour) are not here yet.

/// An sRGB colour with 8 bits per channel.
pub type Rgb = [u8; 3];

/// Relative luminance of a colour, from 0 (black) to 1 (white), as WCAG 2.x defines it.
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
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Text is "large" at 24 CSS px, or 18.66 px when bold (WCAG 1.4.3).
pub fn is_large_text(size_px: f64, bold: bool) -> bool {
    size_px >= 24.0 || (bold && size_px >= 18.66)
}

/// The minimum contrast for text under WCAG 2.2 AA: 4.5 for normal text, 3 for large text.
pub fn min_text_contrast(large: bool) -> f64 {
    if large {
        3.0
    } else {
        4.5
    }
}

/// Smallest side of a pointer target under WCAG 2.2 AA (2.5.8), in CSS px.
pub const MIN_TARGET_PX: f64 = 24.0;

/// One problem a rule found.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    /// The rule that failed, such as `contrast` or `target-size`.
    pub rule: &'static str,
    pub message: String,
}

/// Checks text colour against its background. `large` is from [`is_large_text`].
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
pub fn check_target_size(width_px: f64, height_px: f64) -> Option<Violation> {
    (width_px < MIN_TARGET_PX || height_px < MIN_TARGET_PX).then(|| Violation {
        rule: "target-size",
        message: format!(
            "target is {width_px}x{height_px} px, needs at least {MIN_TARGET_PX}x{MIN_TARGET_PX}"
        ),
    })
}

/// A form control as the accessibility tree reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    /// The role, such as `textbox` or `checkbox`.
    pub role: String,
    /// The accessible name, or nothing when the control has none.
    pub name: Option<String>,
}

/// Returns a violation for each input control that has no accessible name. Buttons and links
/// are not form inputs here; they are covered by the names rule of the tree dump.
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
}
