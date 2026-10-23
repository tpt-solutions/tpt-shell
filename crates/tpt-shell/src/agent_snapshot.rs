// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The snapshot source for the agent's `wait_for` tool (spec §10.4). A snapshot is the page's
//! accessible tree, read by running the agent's snapshot script in the window's page. The platform
//! runs the script and hands back its JSON result; this module turns that result into a snapshot.
//!
//! The platform's script runner must wait for the page's answer on the UI thread. Until one is
//! supplied to the agent setup, no `wait_for` tool is offered.

use tpt_shell_agent::{page, SnapshotSource};

/// Runs `script` in one window's page and returns its JSON result, or `None` when the script gives
/// no value. It waits for the answer on the UI thread, so the message loop keeps running only
/// between snapshots.
pub(crate) type PageScript = Box<dyn FnMut(&str) -> Result<Option<String>, String>>;

/// A snapshot source over a page script. Each call runs the snapshot script once and parses its
/// result; a page that gives no value or invalid JSON is an error, not an empty snapshot.
pub(crate) fn snapshot_source(mut run: PageScript) -> SnapshotSource {
    Box::new(move || {
        let json = run(&page::snapshot_script())?
            .ok_or_else(|| "the page gave no snapshot".to_string())?;
        page::parse_snapshot(&json)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;
    use tpt_shell_agent::{wait_for_tool, Policy, Session};

    const SAVE: &str =
        r#"{"ref":7,"role":"button","name":"Save","bounds":{"x":0,"y":0,"w":1,"h":1}}"#;

    fn page_json(nodes: &str) -> String {
        format!(
            r#"{{"title":"t","path":"https://tpt-app.localhost/","truncated":false,"tree":[{nodes}]}}"#
        )
    }

    /// A fake page whose Save button appears on the `after`-th snapshot. It returns the number of
    /// snapshots taken so far through the counter.
    fn fake_page(after: u32) -> (PageScript, Rc<Cell<u32>>) {
        let taken = Rc::new(Cell::new(0));
        let seen = taken.clone();
        let script: PageScript = Box::new(move |script| {
            assert!(
                script.contains("__tptAgent.snapshot()"),
                "the snapshot script runs"
            );
            let n = seen.get() + 1;
            seen.set(n);
            let nodes = if n >= after { SAVE } else { "" };
            Ok(Some(page_json(nodes)))
        });
        (script, taken)
    }

    #[test]
    fn a_page_result_parses_into_a_snapshot_with_its_nodes() {
        let (script, _) = fake_page(1);
        let mut source = snapshot_source(script);
        let snapshot = source().expect("a snapshot");
        assert_eq!(snapshot.tree.len(), 1);
        assert_eq!(snapshot.tree[0].role, "button");
        assert_eq!(snapshot.tree[0].name, "Save");
    }

    #[test]
    fn a_page_that_gives_no_value_is_an_error_not_an_empty_snapshot() {
        let mut source = snapshot_source(Box::new(|_| Ok(None)));
        let err = source().expect_err("refused");
        assert!(err.contains("no snapshot"), "{err}");
    }

    #[test]
    fn a_page_that_gives_invalid_json_is_refused() {
        let mut source = snapshot_source(Box::new(|_| Ok(Some("not json".into()))));
        let err = source().expect_err("refused");
        assert!(err.contains("not valid"), "{err}");
    }

    #[test]
    fn wait_for_through_a_session_finds_the_element_on_a_later_snapshot() {
        let (script, taken) = fake_page(3);
        let mut session = Session::new(
            vec![wait_for_tool(snapshot_source(script))],
            Policy::default(),
        );
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"wait_for","arguments":{"role":"button","name":"Save","timeout_ms":2000,"poll_ms":10}}}"#;
        let reply = session
            .handle_line(line, Duration::ZERO, &mut |_, _| false)
            .expect("a reply");
        assert!(reply.contains("Save"), "{reply}");
        assert!(!reply.contains("isError\":true"), "{reply}");
        assert_eq!(taken.get(), 3, "the element appeared on the third snapshot");
    }
}
