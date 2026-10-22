// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Native confirmation for agent tool calls (spec §9). The agent session asks through
//! [`tpt_shell_agent::Confirm`]; this answers with a Yes/No message box that the shell owns,
//! so the page and the agent cannot answer it. The default button is No, so a stray Enter
//! refuses the call.

use std::ptr::null_mut;

use serde_json::Value;
use tpt_shell_agent::Confirm;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_TOPMOST, MB_YESNO,
};

use crate::backend::WindowId;
use crate::com::wide;

/// Caption of the confirmation box.
pub const CAPTION: &str = "tpt-shell: agent request";

/// Longest argument text shown in the box. Longer text is cut and marked.
const MAX_SHOWN: usize = 1000;

/// The question shown for a tool call, with its arguments as pretty JSON.
///
/// Support: Full.
pub fn question(command: &str, args: &Value) -> String {
    let mut shown = serde_json::to_string_pretty(args)
        .unwrap_or_else(|_| "(the arguments cannot be shown)".into());
    if shown.chars().count() > MAX_SHOWN {
        shown = shown.chars().take(MAX_SHOWN).collect::<String>() + "...";
    }
    format!(
        "An agent asks to run the tool \"{command}\" with these arguments:\n\n{shown}\n\n\
         Allow it? Choose No unless you started this action."
    )
}

/// Whether a `MessageBoxW` result is a yes. Anything else, including a failed call (0),
/// refuses.
pub fn answer_is_yes(code: i32) -> bool {
    code == IDYES
}

/// Asks the user with a Yes/No box. Owned by `owner` when one is given; otherwise the box is
/// topmost so it cannot hide behind other windows.
///
/// Support: Full on Windows. A box that fails to show refuses the call.
#[derive(Clone, Copy, Debug)]
pub struct NativeConfirm {
    owner: Option<WindowId>,
}

impl NativeConfirm {
    /// A prompt owned by `owner`, or unowned when `None`.
    ///
    /// Support: Full on Windows.
    pub fn new(owner: Option<WindowId>) -> Self {
        NativeConfirm { owner }
    }
}

impl Confirm for NativeConfirm {
    fn confirm(&mut self, command: &str, args: &Value) -> bool {
        let text = wide(&question(command, args));
        let title = wide(CAPTION);
        let (owner, extra) = match self.owner {
            Some(window) => (window.0 as HWND, 0),
            None => (null_mut(), MB_TOPMOST),
        };
        let style = MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | extra;
        // SAFETY: both buffers are NUL-terminated and live across the call; the owner is a
        // window handle or null.
        let code = unsafe { MessageBoxW(owner, text.as_ptr(), title.as_ptr(), style) };
        answer_is_yes(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_question_names_the_tool_and_shows_the_arguments() {
        let text = question("files.delete", &json!({"path": "C:\\data\\a.txt"}));
        assert!(text.contains("files.delete"));
        assert!(text.contains("a.txt"));
    }

    #[test]
    fn long_arguments_are_cut_and_marked() {
        let big = json!({"text": "x".repeat(5000)});
        let text = question("tool", &big);
        assert!(text.contains("..."));
        assert!(text.len() < 2000);
    }

    #[test]
    fn only_yes_allows_the_call() {
        assert!(answer_is_yes(IDYES));
        assert!(!answer_is_yes(7)); // IDNO
        assert!(!answer_is_yes(2)); // IDCANCEL
        assert!(!answer_is_yes(0)); // the box failed to show
    }
}
