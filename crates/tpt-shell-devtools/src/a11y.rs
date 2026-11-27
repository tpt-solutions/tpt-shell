// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The accessibility panel of the overlay: a quick check of the page the developer is looking at.
//!
//! The host asks the page to run [`COLLECT_SCRIPT`] and post its result back. [`check`] reads that
//! result and runs the rules from `tpt-shell-test`, so the overlay shows the same findings the test
//! suite reports. The check is a heuristic: it covers the rules in `tpt_shell_test::a11y` that the
//! page's own data can answer, and it says so in the panel.
//!
//! Support: Partial. Only elements with an `id` are checked, and the rules are heuristics where
//! marked in `tpt_shell_test::a11y`.

use serde_json::{json, Value};
use tpt_shell_test::page_audit::{self, Found};

/// The script the host runs in the page. It returns the page data as a JSON string.
///
/// Support: Partial. See the module documentation.
pub const COLLECT_SCRIPT: &str = page_audit::COLLECT_SCRIPT;

/// One finding of the quick check.
///
/// Support: Partial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: String,
    /// The element id the finding is about.
    pub id: String,
    pub message: String,
}

/// The result of a check: the findings, and whether the page reported it prefers reduced motion.
///
/// Support: Partial.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub prefers_reduced_motion: bool,
}

/// Checks the page data `page_json` (what [`COLLECT_SCRIPT`] returns). Bad data is an error.
///
/// Support: Partial.
pub fn check(page_json: &str) -> Result<Report, String> {
    let page: Value = serde_json::from_str(page_json).map_err(|e| format!("not JSON: {e}"))?;
    let prefers = page
        .get("prefers_reduced")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let found: Vec<Found> = page_audit::audit(&page, prefers)?;
    Ok(Report {
        findings: found
            .into_iter()
            .map(|f| Finding {
                rule: f.rule,
                id: f.id,
                message: f.message,
            })
            .collect(),
        prefers_reduced_motion: prefers,
    })
}

/// The report as JSON, for the overlay page.
///
/// Support: Full.
pub fn to_json(report: &Report) -> Value {
    json!({
        "checked": true,
        "prefersReducedMotion": report.prefers_reduced_motion,
        "findings": report.findings.iter().map(|f| json!({
            "rule": f.rule,
            "id": f.id,
            "message": f.message,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(extra: &str) -> String {
        format!(
            r#"{{"prefers_reduced": false, "elements": [{{"id": "low", "tag": "p", "role": "generic",
            "name": null, "text": true, "fg": [153,153,153], "bg": [255,255,255], "font_px": 16,
            "bold": false, "width": 80, "height": 20, "shown": true, "focusable": false,
            "tab_index": 0, "top": 0, "animations": []}}{extra}]}}"#
        )
    }

    #[test]
    fn a_low_contrast_text_is_a_finding_about_its_element() {
        let report = check(&page("")).expect("check");
        assert_eq!(report.findings.len(), 1, "{report:?}");
        assert_eq!(report.findings[0].rule, "contrast");
        assert_eq!(report.findings[0].id, "low");
        assert!(!report.prefers_reduced_motion);
    }

    #[test]
    fn the_json_lists_the_findings_and_says_it_was_checked() {
        let report = check(&page("")).expect("check");
        let json = to_json(&report);
        assert_eq!(json["checked"], json!(true));
        assert_eq!(json["findings"][0]["id"], json!("low"));
    }

    #[test]
    fn bad_data_is_an_error_not_a_panic() {
        assert!(check("not json").is_err());
        assert!(check("{}").is_err());
    }
}
