// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Registry of stable `TS####` error codes (spec §12, M3). Codes never change meaning once
//! published. `tpt-shell explain <code>` prints the entry for a code.

/// One registered error code.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub code: &'static str,
    pub title: &'static str,
    pub explanation: &'static str,
    pub fix: &'static str,
}

/// Every registered code, in code order.
///
/// Support: Full.
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
    Entry {
        code: "TS0100",
        title: "signing key unavailable",
        explanation: "The audit checkpoint signing key could not be read, or the keystore that holds it refused the request.",
        fix: "Check that the key file exists and is readable by the current user, or that the keystore is unlocked, then retry.",
    },
    Entry {
        code: "TS0101",
        title: "signature invalid",
        explanation: "A checkpoint or detached signature does not match the data and the public key it was checked against.",
        fix: "Check that the file was not changed after signing and that the public key is the one that signed it.",
    },
    Entry {
        code: "TS0102",
        title: "anchor write failed",
        explanation: "The external anchor could not be written, so the audit chain head was not recorded outside the log.",
        fix: "Check that the anchor path exists, is writable, and is on the volume you intended, then retry the checkpoint.",
    },
    Entry {
        code: "TS0103",
        title: "anchor mismatch or malformed anchor",
        explanation: "The anchor does not match the audit chain it was written for, or the anchor file cannot be parsed. The log may have been truncated, replaced, or edited.",
        fix: "Compare the log with its anchor and restore the log from a backup if the anchor is the trusted copy.",
    },
    Entry {
        code: "TS0104",
        title: "audit segments missing or out of order",
        explanation: "A rotated audit log segment is missing, or the segment numbers are not in sequence, so the chain cannot be checked across files.",
        fix: "Restore the missing segment from a backup or the external anchor, then run the audit verify command again.",
    },
    Entry {
        code: "TS0110",
        title: "agent profile invalid",
        explanation: "An [agent] profile is malformed, or one of its route patterns cannot be used. The profile is refused so that a typo cannot widen what the agent may reach.",
        fix: "Fix the reported route pattern or key in the [agent] section. Routes are exact paths, or a prefix ending in '*'.",
    },
    Entry {
        code: "TS0111",
        title: "HTTP token or bind refused",
        explanation: "The local HTTP endpoint was refused: its token is shorter than 32 characters, or the address is not the loopback address.",
        fix: "Use a token of at least 32 characters and bind to 127.0.0.1 only.",
    },
    Entry {
        code: "TS0112",
        title: "route refused",
        explanation: "An agent tried to open a URL or route that the profile's allow-list does not include. Navigation fails closed.",
        fix: "Add the route to the agent profile if the agent should reach it, or use an allowed route.",
    },
    Entry {
        code: "TS0113",
        title: "action not recorded, session stopped",
        explanation: "An agent action could not be written to the audit log, so it was not run and the session was stopped.",
        fix: "Check that the audit log is writable and has space, then start a new agent session.",
    },
    Entry {
        code: "TS0114",
        title: "invoke not allowed",
        explanation: "An agent asked to run a command that the profile does not allow, or that needs confirmation it did not get.",
        fix: "Add the command to the profile's invoke list, or confirm the action when prompted.",
    },
    Entry {
        code: "TS0120",
        title: "deny.toml unreadable",
        explanation: "The licence policy file could not be read or parsed, so the licence gate cannot run.",
        fix: "Check that deny.toml exists in the project root and is valid TOML with an allow list.",
    },
    Entry {
        code: "TS0122",
        title: "licence gate refused",
        explanation: "A dependency has a licence that the policy in deny.toml does not allow. The build is refused.",
        fix: "Replace the dependency, or record a justified exception in deny.toml after review.",
    },
    Entry {
        code: "TS0301",
        title: "test message refused",
        explanation: "The test driver received a message it does not accept. Only the documented commands run, and nothing else is executed.",
        fix: "Use one of the driver's documented commands, or check that the test client and the app are the same version.",
    },
    Entry {
        code: "TS0302",
        title: "test driver timed out",
        explanation: "The app did not answer a test driver request before the deadline.",
        fix: "Check that the app is running and not blocked, or increase the timeout for a slow step.",
    },
    Entry {
        code: "TS0303",
        title: "element not found",
        explanation: "No element matched the role and name in the test query. This is the only test driver error that is retried.",
        fix: "Check the role and name against the accessibility tree, or wait for the page to finish loading.",
    },
    Entry {
        code: "TS0304",
        title: "not supported on this platform",
        explanation: "The test driver or a replay step needs a capability that this platform does not provide yet.",
        fix: "Run the test on a platform that supports it, or skip the step on this platform.",
    },
    Entry {
        code: "TS0305",
        title: "artifact path refused",
        explanation: "A test artifact path points outside the artifact directory, or is not a plain relative path.",
        fix: "Use a plain relative file name inside the artifact directory.",
    },
    Entry {
        code: "TS0306",
        title: "element is ambiguous",
        explanation: "More than one element matched the role and name in the test query, so the driver cannot tell which one to act on. This is not retried, because waiting does not change the match count.",
        fix: "Narrow the query with a more specific name, or give the elements distinct accessible names.",
    },
    Entry {
        code: "TS0307",
        title: "recording not loaded",
        explanation: "The developer overlay could not read the recording it was given: the file is missing, is not a recording, or fails the strict read. The overlay runs without a timeline.",
        fix: "Check the path, and that the file was written by the replay recorder. A lenient read skips damaged lines.",
    },
    Entry {
        code: "TS0308",
        title: "timeline seek refused",
        explanation: "The overlay asked the timeline to move to a position that is not in the recording, or no recording is loaded.",
        fix: "Seek to a position between 0 and the number of events shown in the timeline.",
    },
];

/// Looks up a code. The match ignores ASCII case, so `ts0001` finds `TS0001`.
///
/// Support: Full.
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
