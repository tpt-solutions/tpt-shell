// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The licence check that `tpt-shell doctor` runs (spec §4). It reads the allow-list from
//! `deny.toml` and checks the licence of every third-party crate in the workspace. It only reads:
//! it never edits `deny.toml` or the lock file.
//!
//! The expression rules are the ones the compliance gate uses: `AND` binds tighter than `OR`,
//! parentheses group, `WITH` attaches an exception that is not itself checked, and `/` means
//! `OR`. Anything the grammar does not cover is refused, so the check fails closed.

use tpt_shell_compliance::Component;

/// The allow-list in `deny.toml`, from its `[licenses] allow = [...]` array.
///
/// Support: Full.
pub fn allow_list(deny_toml: &str) -> Result<Vec<String>, String> {
    let table: toml::Table = deny_toml
        .parse()
        .map_err(|e| format!("deny.toml is not valid TOML: {e}"))?;
    let allow = table
        .get("licenses")
        .and_then(|l| l.get("allow"))
        .and_then(toml::Value::as_array)
        .ok_or("deny.toml has no [licenses] allow list")?;
    allow
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| "a licence in deny.toml is not a string".to_string())
        })
        .collect()
}

/// Splits an SPDX-like expression into tokens. `/` becomes `OR`. Returns `None` for characters
/// the grammar does not use.
fn tokenize(expression: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let flush = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    for c in expression.chars() {
        match c {
            c if c.is_whitespace() => flush(&mut word, &mut tokens),
            '(' | ')' => {
                flush(&mut word, &mut tokens);
                tokens.push(c.to_string());
            }
            '/' => {
                flush(&mut word, &mut tokens);
                tokens.push("OR".into());
            }
            c if c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '+' => word.push(c),
            _ => return None,
        }
    }
    flush(&mut word, &mut tokens);
    Some(tokens)
}

/// Whether `expression` is acceptable when only the licences in `allowed` are.
///
/// Support: Full.
pub fn expression_allowed(expression: &str, allowed: &[String]) -> bool {
    let Some(tokens) = tokenize(expression) else {
        return false;
    };
    if tokens.is_empty() {
        return false;
    }
    let mut pos = 0;
    match or_expr(&tokens, &mut pos, allowed) {
        Some(value) => value && pos == tokens.len(),
        None => false,
    }
}

fn or_expr(tokens: &[String], pos: &mut usize, allowed: &[String]) -> Option<bool> {
    let mut value = and_expr(tokens, pos, allowed)?;
    while tokens.get(*pos).map(String::as_str) == Some("OR") {
        *pos += 1;
        value |= and_expr(tokens, pos, allowed)?;
    }
    Some(value)
}

fn and_expr(tokens: &[String], pos: &mut usize, allowed: &[String]) -> Option<bool> {
    let mut value = atom(tokens, pos, allowed)?;
    while tokens.get(*pos).map(String::as_str) == Some("AND") {
        *pos += 1;
        value &= atom(tokens, pos, allowed)?;
    }
    Some(value)
}

fn atom(tokens: &[String], pos: &mut usize, allowed: &[String]) -> Option<bool> {
    let token = tokens.get(*pos)?;
    *pos += 1;
    match token.as_str() {
        "(" => {
            let value = or_expr(tokens, pos, allowed)?;
            if tokens.get(*pos).map(String::as_str) != Some(")") {
                return None;
            }
            *pos += 1;
            Some(value)
        }
        "AND" | "OR" | ")" | "WITH" => None,
        licence => {
            // `X WITH Y`: the exception is not a licence, so only X is checked.
            if tokens.get(*pos).map(String::as_str) == Some("WITH") {
                *pos += 2;
            }
            Some(allowed.iter().any(|a| a == licence))
        }
    }
}

/// One line per third-party crate whose licence is missing or not allowed.
///
/// Support: Full.
pub fn violations(components: &[Component], allowed: &[String]) -> Vec<String> {
    components
        .iter()
        .filter_map(|c| match &c.license {
            None => Some(format!("{} {}: no licence declared", c.name, c.version)),
            Some(expr) if !expression_allowed(expr, allowed) => Some(format!(
                "{} {}: licence `{expr}` is not in the allow list",
                c.name, c.version
            )),
            Some(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        ["MIT", "Unicode-3.0", "BSD-3-Clause", "ISC"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn the_allow_list_is_read_from_deny_toml() {
        let list = allow_list("[licenses]\nversion = 2\nallow = [\n  \"MIT\",\n  \"ISC\",\n]\n")
            .expect("list");
        assert_eq!(list, vec!["MIT".to_string(), "ISC".to_string()]);
        assert!(allow_list("[licenses]\n").is_err());
    }

    #[test]
    fn the_repository_deny_toml_has_the_expected_licences() {
        let text = include_str!("../../../deny.toml");
        let list = allow_list(text).expect("the repository deny.toml parses");
        for expected in ["MIT", "Unicode-3.0", "BSD-3-Clause", "ISC"] {
            assert!(list.iter().any(|l| l == expected), "{expected} missing");
        }
        assert!(!list.iter().any(|l| l.starts_with("Apache")));
    }

    #[test]
    fn an_or_expression_passes_when_one_branch_is_allowed() {
        assert!(expression_allowed("MIT OR Apache-2.0", &allowed()));
        assert!(expression_allowed("Apache-2.0 / MIT", &allowed()));
        assert!(expression_allowed(
            "(MIT OR Apache-2.0) AND Unicode-3.0",
            &allowed()
        ));
    }

    #[test]
    fn an_and_expression_needs_both_sides() {
        assert!(!expression_allowed("MIT AND Apache-2.0", &allowed()));
        assert!(!expression_allowed("dpi", &allowed()));
        assert!(expression_allowed("MIT AND Unicode-3.0", &allowed()));
    }

    #[test]
    fn apache_only_and_copyleft_are_refused() {
        assert!(!expression_allowed("Apache-2.0", &allowed()));
        assert!(!expression_allowed("GPL-3.0-only", &allowed()));
        assert!(!expression_allowed("LGPL-2.1-or-later", &allowed()));
        assert!(!expression_allowed("MPL-2.0", &allowed()));
    }

    #[test]
    fn a_with_exception_checks_only_the_licence() {
        assert!(expression_allowed("MIT WITH LLVM-exception", &allowed()));
        assert!(!expression_allowed(
            "GPL-3.0 WITH Classpath-exception-2.0",
            &allowed()
        ));
    }

    #[test]
    fn malformed_expressions_fail_closed() {
        for bad in [
            "",
            "MIT OR",
            "(MIT",
            "MIT)",
            "MIT & ISC",
            "AND MIT",
            "MIT WITH",
        ] {
            assert!(!expression_allowed(bad, &allowed()), "{bad:?} passed");
        }
    }

    #[test]
    fn violations_name_missing_and_refused_licences() {
        let components = vec![
            Component {
                name: "good".into(),
                version: "1.0.0".into(),
                license: Some("MIT OR Apache-2.0".into()),
                repository: None,
            },
            Component {
                name: "bad".into(),
                version: "2.0.0".into(),
                license: Some("GPL-3.0-only".into()),
                repository: None,
            },
            Component {
                name: "silent".into(),
                version: "3.0.0".into(),
                license: None,
                repository: None,
            },
        ];
        let found = violations(&components, &allowed());
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("bad 2.0.0"));
        assert!(found[1].starts_with("silent 3.0.0"));
    }
}
