// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The app's answers to the test driver (spec §10.6). The app polls its end of the run directory
//! on the UI thread and calls [`serve`] for each request. [`serve`] checks the request (the channel
//! does that), answers it from a [`Host`], and writes the reply. The host is the window: it
//! supplies the page's accessibility nodes, the actions, the capture and the scripts. This module
//! needs no window, so it is tested with a fake host.
//!
//! Element handles are the snapshot references of the page. A handle from a `find` is valid until
//! the page changes; the page refuses a stale one, and that refusal reaches the driver as an
//! error.
//!
//! Support: Full on the file channel, tested here with a fake host. The facade supplies the
//! WebView2 host under the `test-driver` feature.

use serde_json::{json, Value};

use crate::channel::{AppChannel, ChannelError, Command};
use crate::driver::NOT_FOUND;

/// The code for a command the host could not carry out. The driver shows the message.
///
/// Support: Full.
pub const HOST_FAILED: &str = "TS0304";

/// One element of the page's accessibility tree, with the reference that names it.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub reference: u32,
    pub role: String,
    pub name: Option<String>,
}

/// What the app does for the driver. Each method may fail with a message, which the driver
/// receives as a `TS0304` error.
///
/// Support: Full.
pub trait Host {
    /// The page's nodes, from a fresh snapshot.
    fn nodes(&mut self) -> Result<Vec<Node>, String>;
    /// Clicks the element with this reference.
    fn click(&mut self, reference: u32) -> Result<(), String>;
    /// Types text into the element with this reference.
    fn type_text(&mut self, reference: u32, text: &str) -> Result<(), String>;
    /// Presses a key by name.
    fn press(&mut self, key: &str) -> Result<(), String>;
    /// Captures the window as PNG bytes.
    fn capture(&mut self) -> Result<Vec<u8>, String>;
    /// The page's DOM, as JSON.
    fn dom(&mut self) -> Result<Value, String>;
    /// Runs a script in the page and returns its JSON result (test builds only).
    fn eval(&mut self, script: &str) -> Result<Value, String>;
}

fn host_failed(message: String) -> ChannelError {
    ChannelError {
        code: HOST_FAILED,
        message,
    }
}

/// The element a handle names. Handles are references, which the page numbers from 1.
fn reference_of(handle: u64) -> Result<u32, ChannelError> {
    u32::try_from(handle)
        .ok()
        .filter(|r| *r > 0)
        .ok_or_else(|| host_failed(format!("{handle} is not an element handle")))
}

/// Answers one command, except a screenshot, which [`serve`] writes out as an artifact.
///
/// Support: Full.
pub fn answer(host: &mut dyn Host, command: &Command) -> Result<Value, ChannelError> {
    match command {
        Command::Find { role, name } => {
            let nodes = host.nodes().map_err(host_failed)?;
            let found: Vec<&Node> = nodes
                .iter()
                .filter(|n| {
                    n.role == *role
                        && name
                            .as_ref()
                            .is_none_or(|want| n.name.as_deref() == Some(want.as_str()))
                })
                .collect();
            match found.first() {
                None => Err(ChannelError {
                    code: NOT_FOUND,
                    message: format!("no element has role {role:?}"),
                }),
                Some(first) => Ok(json!({ "handle": first.reference, "matches": found.len() })),
            }
        }
        Command::Click { handle } => {
            host.click(reference_of(*handle)?).map_err(host_failed)?;
            Ok(json!({ "clicked": handle }))
        }
        Command::TypeText { handle, text } => {
            host.type_text(reference_of(*handle)?, text)
                .map_err(host_failed)?;
            Ok(json!({ "typed": text.chars().count() }))
        }
        Command::Press { key } => {
            host.press(key).map_err(host_failed)?;
            Ok(json!({ "pressed": key }))
        }
        Command::Screenshot { .. } => Err(host_failed(
            "a screenshot is written by serve, not by answer".into(),
        )),
        Command::DomSnapshot => host.dom().map_err(host_failed),
        Command::A11yTree => {
            // The page's own accessibility nodes. The native tree (UI Automation and its
            // equivalents) is read by the platform crates, which the host does not reach yet.
            let nodes = host.nodes().map_err(host_failed)?;
            Ok(json!({
                "nodes": nodes
                    .iter()
                    .map(|n| json!({ "role": n.role, "name": n.name }))
                    .collect::<Vec<Value>>(),
            }))
        }
        Command::Eval { script } => host.eval(script).map_err(host_failed),
    }
}

/// Serves one pending request, if there is one, and reports whether it did. A request that fails
/// the channel's checks is already answered by the channel and is skipped here.
///
/// Support: Full.
pub fn serve(channel: &mut AppChannel, host: &mut dyn Host) -> bool {
    let Some(polled) = channel.poll() else {
        return false;
    };
    let Ok(request) = polled else {
        return true;
    };
    let result = match &request.command {
        Command::Screenshot { name } => host
            .capture()
            .map_err(host_failed)
            .and_then(|png| channel.write_artifact(name, &png))
            .map(|()| json!({ "artifact": format!("{name}.png") })),
        command => answer(host, command),
    };
    // A reply that cannot be written is the driver's timeout to report, not the app's.
    let _ = channel.respond(request.seq, &result);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::FileChannel;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A host with a fixed page: one button "Save" and one text field "Name".
    #[derive(Default)]
    struct Page {
        clicked: Vec<u32>,
        typed: Vec<(u32, String)>,
    }

    impl Host for Page {
        fn nodes(&mut self) -> Result<Vec<Node>, String> {
            Ok(vec![
                Node {
                    reference: 1,
                    role: "button".into(),
                    name: Some("Save".into()),
                },
                Node {
                    reference: 2,
                    role: "textbox".into(),
                    name: Some("Name".into()),
                },
            ])
        }
        fn click(&mut self, reference: u32) -> Result<(), String> {
            self.clicked.push(reference);
            Ok(())
        }
        fn type_text(&mut self, reference: u32, text: &str) -> Result<(), String> {
            self.typed.push((reference, text.to_string()));
            Ok(())
        }
        fn press(&mut self, _key: &str) -> Result<(), String> {
            Ok(())
        }
        fn capture(&mut self) -> Result<Vec<u8>, String> {
            Ok(b"png".to_vec())
        }
        fn dom(&mut self) -> Result<Value, String> {
            Ok(json!({ "tag": "html" }))
        }
        fn eval(&mut self, script: &str) -> Result<Value, String> {
            Ok(json!(script.len()))
        }
    }

    fn find(role: &str, name: Option<&str>) -> Command {
        Command::Find {
            role: role.into(),
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn find_returns_the_first_match_and_how_many_there_are() {
        let mut page = Page::default();
        assert_eq!(
            answer(&mut page, &find("button", Some("Save"))).unwrap(),
            json!({ "handle": 1, "matches": 1 })
        );
        let missing = answer(&mut page, &find("button", Some("Quit"))).unwrap_err();
        assert_eq!(missing.code, NOT_FOUND);
    }

    #[test]
    fn click_and_type_act_on_the_reference_the_handle_names() {
        let mut page = Page::default();
        answer(&mut page, &Command::Click { handle: 1 }).unwrap();
        answer(
            &mut page,
            &Command::TypeText {
                handle: 2,
                text: "Ada".into(),
            },
        )
        .unwrap();
        assert_eq!(page.clicked, [1]);
        assert_eq!(page.typed, [(2, "Ada".to_string())]);
        // A handle of zero, or one too large for a reference, never reaches the page.
        assert_eq!(
            answer(&mut page, &Command::Click { handle: 0 })
                .unwrap_err()
                .code,
            HOST_FAILED
        );
    }

    #[test]
    fn a11y_tree_reports_the_page_nodes_and_is_not_the_native_tree() {
        let mut page = Page::default();
        let tree = answer(&mut page, &Command::A11yTree).unwrap();
        assert_eq!(tree["nodes"][0]["role"], json!("button"));
        assert_eq!(tree["nodes"].as_array().map(Vec::len), Some(2));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tpt-runner-{name}-{}-{}",
            std::process::id(),
            crate::channel::Token::generate().as_str()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_driver_call_is_answered_through_the_channel_and_a_screenshot_is_written() {
        let base = scratch("round-trip");
        let mut driver = FileChannel::create(&base).unwrap();
        let mut app = AppChannel::open(driver.dir(), driver.token().as_str()).unwrap();
        let mut page = Page::default();

        // The driver makes its calls on this thread, and the app serves them here, as the UI
        // thread would. Each call returns only after its reply is written.
        let deadline = Instant::now() + Duration::from_secs(10);
        let client = std::thread::spawn(move || {
            let found = driver
                .call(&find("button", Some("Save")), Duration::from_secs(10))
                .unwrap();
            let shot = driver
                .call(
                    &Command::Screenshot {
                        name: "home".into(),
                    },
                    Duration::from_secs(10),
                )
                .unwrap();
            (driver, found, shot)
        });
        while !client.is_finished() {
            assert!(Instant::now() < deadline, "the driver never finished");
            serve(&mut app, &mut page);
            std::thread::sleep(Duration::from_millis(2));
        }
        let (driver, found, shot) = client.join().unwrap();
        assert_eq!(found["handle"], json!(1));
        assert_eq!(shot["artifact"], json!("home.png"));
        assert_eq!(driver.read_artifact("home.png").unwrap(), b"png");
        let _ = fs::remove_dir_all(&base);
    }
}
