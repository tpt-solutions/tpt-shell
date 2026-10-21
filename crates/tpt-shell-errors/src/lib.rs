// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Registry of stable `TS####` error codes (spec §12, M3). Codes never change meaning once
//! published. `tpt-shell explain <code>` prints the entry for a code.

/// One registered error code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub code: &'static str,
    pub title: &'static str,
    pub explanation: &'static str,
    pub fix: &'static str,
}

/// Every registered code, in code order.
pub const REGISTRY: &[Entry] = &[
    Entry {
        code: "TS0001",
        title: "manifest syntax error",
        explanation: "tpt-shell.toml is not valid TOML, or a value has the wrong type.",
        fix: "Open the reported line and fix the syntax or the value type.",
    },
    Entry {
        code: "TS0002",
        title: "unknown manifest key",
        explanation: "A key in tpt-shell.toml is not part of the manifest schema. Usually a typo.",
        fix: "Rename the key to the spelling the schema uses, or remove it.",
    },
    Entry {
        code: "TS0003",
        title: "invalid manifest value",
        explanation: "A value is present but not allowed: an empty or overlong name, a window size that is not positive, a duplicate window name, or a malformed capability name.",
        fix: "Use 1-64 ASCII letters, digits, '-' or '_' for names; positive numbers for sizes; unique window names; dotted lowercase capability names such as fs.read.",
    },
    Entry {
        code: "TS0004",
        title: "missing manifest key",
        explanation: "A required section or key is absent from tpt-shell.toml.",
        fix: "Add the missing section or key. Every manifest needs [app] with a name.",
    },
];

/// Looks up a code. The match ignores ASCII case, so `ts0001` finds `TS0001`.
pub fn lookup(code: &str) -> Option<&'static Entry> {
    REGISTRY
        .iter()
        .find(|e| e.code.eq_ignore_ascii_case(code.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        for (i, entry) in REGISTRY.iter().enumerate() {
            assert_eq!(entry.code.len(), 6, "{}", entry.code);
            assert!(entry.code.starts_with("TS"));
            assert!(entry.code[2..].chars().all(|c| c.is_ascii_digit()));
            assert!(
                REGISTRY[..i].iter().all(|other| other.code != entry.code),
                "{} is registered twice",
                entry.code
            );
        }
    }

    #[test]
    fn every_entry_has_an_explanation_and_a_fix() {
        for entry in REGISTRY {
            assert!(!entry.title.is_empty());
            assert!(!entry.explanation.is_empty(), "{}", entry.code);
            assert!(!entry.fix.is_empty(), "{}", entry.code);
        }
    }

    #[test]
    fn lookup_ignores_case_and_whitespace() {
        assert_eq!(lookup(" ts0002 ").map(|e| e.code), Some("TS0002"));
        assert!(lookup("TS9999").is_none());
    }
}
