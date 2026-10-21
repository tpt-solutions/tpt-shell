// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Native consent prompt (spec §9). The question is asked in a message box the shell owns, never
//! in web content, so a page cannot answer it. The default button is No: a stray Enter refuses.

use tpt_shell_capabilities::ConsentPrompt;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, MessageBoxW, PostMessageW, IDYES, MB_DEFBUTTON2, MB_ICONQUESTION, MB_YESNO,
    WM_COMMAND,
};

use crate::backend::WindowId;
use crate::com::wide;

/// Caption of the consent dialog. Tests find the dialog by it.
pub const CAPTION: &str = "tpt-shell: permission request";

/// The question shown for a request, in plain words.
pub fn question(window: &str, capability: &str) -> String {
    format!(
        "The window \"{window}\" asks to use \"{capability}\".\n\n\
         Allow it? Your answer is remembered for this window."
    )
}

/// Asks the user in a message box owned by the window that asked.
#[derive(Clone, Copy, Debug)]
pub struct NativeConsent {
    owner: WindowId,
}

impl NativeConsent {
    /// A prompt whose dialog is owned by `owner`, the window whose page made the request.
    pub fn for_window(owner: WindowId) -> Self {
        NativeConsent { owner }
    }
}

impl ConsentPrompt for NativeConsent {
    fn ask(&mut self, window: &str, capability: &str) -> bool {
        let text = wide(&question(window, capability));
        let caption = wide(CAPTION);
        // SAFETY: both strings are NUL-terminated and live across the call. The owner is a
        // window this backend created, and the call runs on the UI thread that owns it.
        let answer = unsafe {
            MessageBoxW(
                self.owner.0 as HWND,
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
            )
        };
        answer == IDYES
    }
}

/// Answers the next consent dialog with `answer` (`IDYES` or `IDNO`) from a helper thread.
/// For tests: the dialog is modal, so the thread that shows it cannot answer it itself.
#[doc(hidden)]
pub fn answer_next_dialog(answer: i32) -> std::thread::JoinHandle<bool> {
    std::thread::spawn(move || {
        let class = wide("#32770");
        let caption = wide(CAPTION);
        for _ in 0..400 {
            // SAFETY: both strings are NUL-terminated and live across the call.
            let dialog = unsafe { FindWindowW(class.as_ptr(), caption.as_ptr()) };
            if !dialog.is_null() {
                // SAFETY: `dialog` is a window handle found just above. Posting a command with
                // the button's id ends the message box with that answer.
                unsafe { PostMessageW(dialog, WM_COMMAND, answer as WPARAM, 0 as LPARAM) };
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{interactive_desktop_available, HiddenWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::IDNO;

    #[test]
    fn the_question_names_the_window_and_the_capability() {
        let text = question("main", "fs.read");
        assert!(text.contains("\"main\""));
        assert!(text.contains("\"fs.read\""));
        assert!(text.contains("remembered"));
    }

    /// Shows the real dialog and answers it from another thread.
    fn ask_with_answer(answer: i32) -> Option<bool> {
        if !interactive_desktop_available() {
            return None;
        }
        // One dialog at a time per process, as for the WebView2 runtime tests.
        let _serial = crate::window::runtime_test_lock();
        let owner = HiddenWindow::new().expect("owner window");
        let clicker = answer_next_dialog(answer);
        let mut prompt = NativeConsent::for_window(WindowId(owner.hwnd() as isize));
        let granted = prompt.ask("main", "fs.read");
        assert!(clicker.join().unwrap(), "the consent dialog never appeared");
        Some(granted)
    }

    #[test]
    fn yes_grants_and_no_refuses() {
        if let Some(granted) = ask_with_answer(IDYES) {
            assert!(granted, "Yes must grant");
        }
        if let Some(granted) = ask_with_answer(IDNO) {
            assert!(!granted, "No must refuse");
        }
    }
}
