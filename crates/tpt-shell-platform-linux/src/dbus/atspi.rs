// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Accessibility tree dump over AT-SPI (todo M5 decision 2).
//!
//! The dump asks the accessibility bus (found through `org.a11y.Bus.GetAddress` on the session
//! bus) for the applications the registry lists, keeps those named `app_name`, and walks their
//! children. Each line is the role and the name of one node, indented by depth.
//!
//! Partial: only roles and names are read. States, bounds, values and actions are not, and an
//! application shows up only when its toolkit's accessibility bridge is running (GTK and
//! WebKitGTK load it when AT-SPI is enabled).

use tpt_shell_core::{Error, Result};

use super::bus::{connect_address, session, Connection, Stream};
use super::wire::Value;

const A11Y_BUS: &str = "org.a11y.Bus";
const A11Y_BUS_PATH: &str = "/org/a11y/bus";
const REGISTRY: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";

/// The text of a dump and how much of the tree it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dump {
    pub text: String,
    /// Nodes written.
    pub nodes: usize,
    /// True when `max_nodes` stopped the walk early.
    pub truncated: bool,
}

/// Dumps the accessibility tree of the application named `app_name`. With `None`, dumps every
/// application the registry lists. `max_depth` and `max_nodes` bound the output.
pub fn dump(app_name: Option<&str>, max_depth: usize, max_nodes: usize) -> Result<Dump> {
    let mut session_conn = session()?;
    let reply = session_conn.call(A11Y_BUS, A11Y_BUS_PATH, A11Y_BUS, "GetAddress", vec![])?;
    let address = reply
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Backend("the accessibility bus gave no address".into()))?
        .to_string();
    drop(session_conn);
    let mut conn = connect_address(&address)?;

    let apps = children(&mut conn, REGISTRY, ROOT_PATH)?;
    let mut out = Dump {
        text: String::new(),
        nodes: 0,
        truncated: false,
    };
    for (bus, path) in apps {
        if out.nodes >= max_nodes {
            out.truncated = true;
            break;
        }
        let name = name_of(&mut conn, &bus, &path).unwrap_or_default();
        if app_name.is_some_and(|wanted| wanted != name) {
            continue;
        }
        walk(&mut conn, &bus, &path, 0, max_depth, max_nodes, &mut out)?;
    }
    Ok(out)
}

fn walk(
    conn: &mut Connection<Stream>,
    bus: &str,
    path: &str,
    depth: usize,
    max_depth: usize,
    max_nodes: usize,
    out: &mut Dump,
) -> Result<()> {
    if out.nodes >= max_nodes {
        out.truncated = true;
        return Ok(());
    }
    let role = role_of(conn, bus, path).unwrap_or_else(|| "unknown".to_string());
    let name = name_of(conn, bus, path).unwrap_or_default();
    out.text
        .push_str(&format!("{}{role}: {name}\n", "  ".repeat(depth)));
    out.nodes += 1;
    if depth >= max_depth {
        return Ok(());
    }
    for (child_bus, child_path) in children(conn, bus, path).unwrap_or_default() {
        walk(
            conn,
            &child_bus,
            &child_path,
            depth + 1,
            max_depth,
            max_nodes,
            out,
        )?;
    }
    Ok(())
}

/// The `(bus name, object path)` pairs of an accessible's children.
fn children(conn: &mut Connection<Stream>, bus: &str, path: &str) -> Result<Vec<(String, String)>> {
    let reply = conn.call(bus, path, ACCESSIBLE, "GetChildren", vec![])?;
    let items = reply
        .first()
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Backend("an accessible gave no children list".into()))?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let fields = item.as_struct()?;
            Some((
                fields.first()?.as_str()?.to_string(),
                fields.get(1)?.as_str()?.to_string(),
            ))
        })
        .collect())
}

fn role_of(conn: &mut Connection<Stream>, bus: &str, path: &str) -> Option<String> {
    let reply = conn
        .call(bus, path, ACCESSIBLE, "GetRoleName", vec![])
        .ok()?;
    reply.first()?.as_str().map(str::to_string)
}

fn name_of(conn: &mut Connection<Stream>, bus: &str, path: &str) -> Option<String> {
    let reply = conn
        .call(
            bus,
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            vec![Value::str(ACCESSIBLE), Value::str("Name")],
        )
        .ok()?;
    reply.first()?.unwrap_variant().as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dump_is_unsupported_or_fails_cleanly_without_the_bus() {
        // Without the feature this is Unsupported; with it, an absent session bus is an error.
        // Either way it must not panic or hang.
        let _ = dump(Some("nobody"), 2, 10);
    }
}
