// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! MCP resources (spec §10.4): the semantic tree of the page and the tail of the audit file.
//!
//! A resource is read-only text. The session marks every read as untrusted content, as it does
//! for tool results. Resources are read through the same gates as tools (kill switch and session
//! limit), and each read is recorded.

use std::path::PathBuf;

use crate::page::{render, Snapshot};

/// URI of the semantic tree of the current page.
pub const TREE_URI: &str = "tpt-shell://page/tree";
/// URI of the last lines of the audit file.
pub const AUDIT_TAIL_URI: &str = "tpt-shell://audit/tail";

/// One resource a session offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: String,
}

/// Something that offers resources. `read` returns the text of one, or why it cannot.
pub trait ResourceProvider {
    fn list(&self) -> Vec<Resource>;
    fn read(&mut self, uri: &str) -> Result<String, String>;
}

/// The semantic tree, rendered as the agent sees it. `take` produces a fresh snapshot.
pub struct PageTree {
    take: Box<dyn FnMut() -> Result<Snapshot, String>>,
}

impl PageTree {
    pub fn new(take: Box<dyn FnMut() -> Result<Snapshot, String>>) -> Self {
        PageTree { take }
    }
}

impl ResourceProvider for PageTree {
    fn list(&self) -> Vec<Resource> {
        vec![Resource {
            uri: TREE_URI.into(),
            name: "page tree".into(),
            description: "The accessible tree of the current page, with snapshot references."
                .into(),
        }]
    }

    fn read(&mut self, uri: &str) -> Result<String, String> {
        if uri != TREE_URI {
            return Err(format!("no resource `{uri}`"));
        }
        let snapshot = (self.take)()?;
        Ok(render(&snapshot))
    }
}

/// The last `lines` lines of an audit file.
pub struct AuditTail {
    path: PathBuf,
    lines: usize,
}

impl AuditTail {
    pub fn new(path: impl Into<PathBuf>, lines: usize) -> Self {
        AuditTail {
            path: path.into(),
            lines: lines.max(1),
        }
    }
}

impl ResourceProvider for AuditTail {
    fn list(&self) -> Vec<Resource> {
        vec![Resource {
            uri: AUDIT_TAIL_URI.into(),
            name: "audit tail".into(),
            description: "The last records of the audit file, as JSON Lines.".into(),
        }]
    }

    fn read(&mut self, uri: &str) -> Result<String, String> {
        if uri != AUDIT_TAIL_URI {
            return Err(format!("no resource `{uri}`"));
        }
        let text = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("cannot read the audit file: {e}"))?;
        let all: Vec<&str> = text.lines().collect();
        let start = all.len().saturating_sub(self.lines);
        Ok(all[start..].join("\n") + "\n")
    }
}

/// Several providers in one. Their resources are listed in order; a URI goes to the first
/// provider that offers it.
#[derive(Default)]
pub struct Resources {
    parts: Vec<Box<dyn ResourceProvider>>,
}

impl Resources {
    pub fn new(parts: Vec<Box<dyn ResourceProvider>>) -> Self {
        Resources { parts }
    }
}

impl ResourceProvider for Resources {
    fn list(&self) -> Vec<Resource> {
        self.parts.iter().flat_map(|p| p.list()).collect()
    }

    fn read(&mut self, uri: &str) -> Result<String, String> {
        for part in &mut self.parts {
            if part.list().iter().any(|r| r.uri == uri) {
                return part.read(uri);
            }
        }
        Err(format!("no resource `{uri}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{Bounds, Node};
    use std::collections::BTreeMap;

    fn snapshot() -> Snapshot {
        Snapshot {
            title: "Home".into(),
            path: "app://x/index.html".into(),
            truncated: false,
            tree: vec![Node {
                ref_id: 1,
                role: "button".into(),
                name: "Start".into(),
                value: None,
                states: BTreeMap::new(),
                bounds: Bounds {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1,
                },
                children: vec![],
            }],
        }
    }

    #[test]
    fn the_tree_resource_renders_the_snapshot_with_untrusted_markers() {
        let mut tree = PageTree::new(Box::new(|| Ok(snapshot())));
        assert_eq!(tree.list()[0].uri, TREE_URI);
        let text = tree.read(TREE_URI).unwrap();
        assert!(text.contains("[ref 1] button <untrusted>Start</untrusted>"));
        assert!(tree.read("tpt-shell://other").is_err());
    }

    #[test]
    fn the_audit_tail_returns_the_last_lines() {
        let dir = std::env::temp_dir().join(format!(
            "tpt-agent-tail-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let mut tail = AuditTail::new(&path, 2);
        assert_eq!(tail.read(AUDIT_TAIL_URI).unwrap(), "two\nthree\n");
        assert!(tail.read(TREE_URI).is_err());
    }

    #[test]
    fn a_combined_provider_routes_each_uri_to_its_owner() {
        let mut all = Resources::new(vec![
            Box::new(PageTree::new(Box::new(|| Ok(snapshot())))),
            Box::new(AuditTail::new("does-not-matter.jsonl", 1)),
        ]);
        assert_eq!(all.list().len(), 2);
        assert!(all.read(TREE_URI).is_ok());
        assert!(all.read("tpt-shell://nothing").is_err());
    }
}
