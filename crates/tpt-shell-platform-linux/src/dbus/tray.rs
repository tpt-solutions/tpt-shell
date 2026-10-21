// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Tray icon through the StatusNotifier protocol (`org.kde.StatusNotifierItem`), the
//! freedesktop way that KDE, GNOME (with the AppIndicator extension) and most others read.
//!
//! Partial: the icon is named by its freedesktop icon name (no pixmap), the tooltip is the
//! title, a left click (`Activate`) reaches the callback, and there is no menu: the item says
//! it has no DBusMenu, so right clicks do nothing. A desktop with no StatusNotifier host
//! (a watcher) reports an error from [`Tray::start`]. There is no GtkStatusIcon fallback.
//!
//! The item is served on its own connection, on a thread. Registration uses a second
//! connection, so the watcher's property reads during registration are answered.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tpt_shell_core::{Error, Result};

use super::bus::{session, Connection, Stream, Transport};
use super::wire::{Kind, Message, Value};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const ITEM_PATH: &str = "/StatusNotifierItem";
const NO_MENU: &str = "/NO_DBUSMENU";
/// How often the serve loop checks for a stop request.
const POLL: Duration = Duration::from_millis(200);

/// What the tray shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayOptions {
    /// Identifies the item to the host. Letters, digits, `-`, `_` or `.`.
    pub app_id: String,
    /// The title, also used as the tooltip.
    pub title: String,
    /// A freedesktop icon name, such as `utilities-terminal`.
    pub icon_name: String,
}

/// A running tray item. Dropping it removes the item from the host and stops the thread.
pub struct Tray {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    /// Shows the item. `on_activate(x, y)` runs on the tray thread for each left click.
    pub fn start(
        options: TrayOptions,
        on_activate: Box<dyn FnMut(i32, i32) + Send>,
    ) -> Result<Tray> {
        validate(&options)?;
        let mut serving = session()?;
        serving.set_read_timeout(Some(POLL))?;
        let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        let granted = serving.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            vec![Value::str(&name), Value::Uint32(0)],
        )?;
        if !matches!(granted.first().and_then(Value::as_u32), Some(1 | 4)) {
            return Err(Error::Backend(
                "another tray item already owns this item name".into(),
            ));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let props = Props {
            id: options.app_id.clone(),
            title: options.title.clone(),
            icon_name: options.icon_name.clone(),
        };
        let mut on_activate = on_activate;
        let thread = thread::Builder::new()
            .name("tpt-shell-tray".into())
            .spawn(move || serve(&mut serving, &props, &thread_stop, &mut on_activate))
            .map_err(|e| Error::Backend(format!("cannot start the tray thread: {e}")))?;
        let tray = Tray {
            stop,
            thread: Some(thread),
        };

        // Register with the host. If this fails, dropping `tray` stops the serving thread.
        let mut registration = session()?;
        registration
            .call(
                WATCHER,
                WATCHER_PATH,
                WATCHER,
                "RegisterStatusNotifierItem",
                vec![Value::str(&name)],
            )
            .map_err(|_| {
                Error::Backend(
                    "no tray host is running (the StatusNotifierWatcher service is missing)".into(),
                )
            })?;
        Ok(tray)
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn validate(options: &TrayOptions) -> Result<()> {
    let id_ok = !options.app_id.is_empty()
        && options
            .app_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !id_ok {
        return Err(Error::Backend(
            "the tray app id may use only letters, digits, '-', '_' or '.'".into(),
        ));
    }
    if options.title.len() > 256 || options.icon_name.len() > 256 {
        return Err(Error::Backend(
            "the tray title and icon name are limited to 256 bytes".into(),
        ));
    }
    Ok(())
}

/// The fixed properties of the item.
struct Props {
    id: String,
    title: String,
    icon_name: String,
}

impl Props {
    fn get(&self, iface: &str, name: &str) -> Option<Value> {
        if iface != ITEM_IFACE {
            return None;
        }
        Some(match name {
            "Category" => Value::str("ApplicationStatus"),
            "Id" => Value::str(&self.id),
            "Title" => Value::str(&self.title),
            "Status" => Value::str("Active"),
            "WindowId" => Value::Uint32(0),
            "IconName" => Value::str(&self.icon_name),
            "OverlayIconName" | "AttentionIconName" => Value::str(""),
            "IconPixmap" | "OverlayIconPixmap" | "AttentionIconPixmap" => {
                Value::Array("(iiay)".to_string(), vec![])
            }
            "ToolTip" => Value::Struct(vec![
                Value::str(""),
                Value::Array("(iiay)".to_string(), vec![]),
                Value::str(&self.title),
                Value::str(""),
            ]),
            "ItemIsMenu" => Value::Bool(false),
            "Menu" => Value::object_path(NO_MENU),
            _ => return None,
        })
    }

    fn all(&self) -> Vec<(&'static str, Value)> {
        const NAMES: [&str; 12] = [
            "Category",
            "Id",
            "Title",
            "Status",
            "WindowId",
            "IconName",
            "OverlayIconName",
            "AttentionIconName",
            "IconPixmap",
            "ToolTip",
            "ItemIsMenu",
            "Menu",
        ];
        NAMES
            .iter()
            .filter_map(|n| self.get(ITEM_IFACE, n).map(|v| (*n, v)))
            .collect()
    }
}

/// The serve loop. Runs until `stop` is set or the connection fails.
fn serve(
    conn: &mut Connection<Stream>,
    props: &Props,
    stop: &AtomicBool,
    on_activate: &mut dyn FnMut(i32, i32),
) {
    while !stop.load(Ordering::SeqCst) {
        match conn.poll_message() {
            Ok(Some(msg)) => {
                if let Err(e) = handle(conn, &msg, props, on_activate) {
                    eprintln!("tpt-shell: tray: {e}");
                    return;
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("tpt-shell: tray stopped: {e}");
                return;
            }
        }
    }
}

/// Answers one incoming message. Only method calls get a reply.
fn handle<S: Transport>(
    conn: &mut Connection<S>,
    msg: &Message,
    props: &Props,
    on_activate: &mut dyn FnMut(i32, i32),
) -> Result<()> {
    if msg.kind != Kind::MethodCall {
        return Ok(());
    }
    let path = msg.header.path.as_deref().unwrap_or_default();
    let iface = msg.header.interface.as_deref().unwrap_or_default();
    let member = msg.header.member.as_deref().unwrap_or_default();
    let reply = if path != ITEM_PATH {
        Message::error(
            msg,
            "org.freedesktop.DBus.Error.UnknownObject",
            "no such object",
        )
    } else {
        match (iface, member) {
            ("org.freedesktop.DBus.Properties", "Get") => {
                let name = msg.body.get(1).and_then(Value::as_str).unwrap_or_default();
                let iface_name = msg.body.first().and_then(Value::as_str).unwrap_or_default();
                match props.get(iface_name, name) {
                    Some(v) => Message::method_return(msg, vec![Value::variant(v)]),
                    None => Message::error(msg, "org.freedesktop.DBus.Error.UnknownProperty", name),
                }
            }
            ("org.freedesktop.DBus.Properties", "GetAll") => {
                let dict = Value::dict_sv(props.all());
                Message::method_return(msg, vec![dict])
            }
            (ITEM_IFACE, "Activate") => {
                let x = msg.body.first().and_then(|v| match v {
                    Value::Int32(n) => Some(*n),
                    _ => None,
                });
                let y = msg.body.get(1).and_then(|v| match v {
                    Value::Int32(n) => Some(*n),
                    _ => None,
                });
                on_activate(x.unwrap_or(0), y.unwrap_or(0));
                Message::method_return(msg, vec![])
            }
            (ITEM_IFACE, "SecondaryActivate" | "ContextMenu" | "Scroll") => {
                Message::method_return(msg, vec![])
            }
            ("org.freedesktop.DBus.Introspectable", "Introspect") => {
                Message::method_return(msg, vec![Value::str(INTROSPECTION)])
            }
            ("org.freedesktop.DBus.Peer", "Ping") => Message::method_return(msg, vec![]),
            _ => Message::error(msg, "org.freedesktop.DBus.Error.UnknownMethod", member),
        }
    };
    let mut reply = reply;
    conn.send(&mut reply).map(|_| ())
}

const INTROSPECTION: &str = r#"<node>
 <interface name="org.kde.StatusNotifierItem">
  <method name="Activate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
  <method name="SecondaryActivate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
  <method name="ContextMenu"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
  <method name="Scroll"><arg name="delta" type="i" direction="in"/><arg name="orientation" type="s" direction="in"/></method>
 </interface>
</node>"#;

#[cfg(test)]
mod tests {
    use super::super::bus::test_support::ScriptedBus;
    use super::*;

    fn props() -> Props {
        Props {
            id: "demo".into(),
            title: "Demo".into(),
            icon_name: "utilities-terminal".into(),
        }
    }

    fn call(member: &str, iface: &str, body: Vec<Value>) -> Message {
        let mut m = Message::method_call("demo", ITEM_PATH, iface, member, body);
        m.serial = 9;
        m.header.sender = Some(":1.7".into());
        m
    }

    /// A connection whose stream was already used for the handshake; only the output is checked.
    fn connection() -> Connection<ScriptedBus> {
        let mut hello = Message::method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "x",
            "Hello",
            vec![],
        );
        hello.serial = 1;
        let reply = Message::method_return(&hello, vec![Value::str(":1.1")]);
        let mut conn = Connection::authenticate(ScriptedBus::new(&[reply]), "1000").unwrap();
        conn.stream_mut().written.clear();
        conn
    }

    fn sent_reply(conn: &mut Connection<ScriptedBus>) -> Message {
        let bytes = std::mem::take(&mut conn.stream_mut().written);
        Message::decode(&bytes).expect("a reply was sent")
    }

    #[test]
    fn properties_are_answered_with_a_variant() {
        let mut conn = connection();
        let msg = call(
            "Get",
            "org.freedesktop.DBus.Properties",
            vec![Value::str(ITEM_IFACE), Value::str("Title")],
        );
        handle(&mut conn, &msg, &props(), &mut |_, _| {}).unwrap();
        let reply = sent_reply(&mut conn);
        assert_eq!(reply.kind, Kind::MethodReturn);
        assert_eq!(
            reply.body.first().map(Value::unwrap_variant),
            Some(&Value::str("Demo"))
        );
    }

    #[test]
    fn a_left_click_reaches_the_callback_with_its_position() {
        let mut conn = connection();
        let mut seen = None;
        let msg = call(
            "Activate",
            ITEM_IFACE,
            vec![Value::Int32(10), Value::Int32(20)],
        );
        handle(&mut conn, &msg, &props(), &mut |x, y| seen = Some((x, y))).unwrap();
        assert_eq!(seen, Some((10, 20)));
        assert_eq!(sent_reply(&mut conn).kind, Kind::MethodReturn);
    }

    #[test]
    fn unknown_methods_get_an_error_reply() {
        let mut conn = connection();
        let msg = call("Frobnicate", ITEM_IFACE, vec![]);
        handle(&mut conn, &msg, &props(), &mut |_, _| {}).unwrap();
        assert_eq!(sent_reply(&mut conn).kind, Kind::Error);
    }

    #[test]
    fn the_item_declares_no_menu() {
        assert_eq!(
            props().get(ITEM_IFACE, "Menu"),
            Some(Value::object_path(NO_MENU))
        );
        assert_eq!(
            props().get(ITEM_IFACE, "ItemIsMenu"),
            Some(Value::Bool(false))
        );
    }

    #[test]
    fn options_are_validated() {
        let good = TrayOptions {
            app_id: "demo".into(),
            title: "Demo".into(),
            icon_name: "x".into(),
        };
        assert!(validate(&good).is_ok());
        let bad = TrayOptions {
            app_id: "a/b".into(),
            ..good
        };
        assert!(validate(&bad).is_err());
    }
}
