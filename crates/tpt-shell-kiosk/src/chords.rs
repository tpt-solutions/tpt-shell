// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Which key presses a kiosk blocks (spec §10.1): Alt+Tab, the Win key, Alt+F4 and Ctrl+Esc.
//! The rule is a pure function of the key code and the modifier state, so it is tested on every
//! platform. The Windows low-level keyboard hook (see `hook_windows`) calls it.

/// Virtual-key codes (Windows `VK_*` values).
///
/// Support: Full.
pub const VK_TAB: u32 = 0x09;
/// Support: Full.
pub const VK_ESCAPE: u32 = 0x1B;
/// Support: Full.
pub const VK_F4: u32 = 0x73;
/// Support: Full.
pub const VK_LWIN: u32 = 0x5B;
/// Support: Full.
pub const VK_RWIN: u32 = 0x5C;

/// Whether a key-down with this key and modifier state is blocked.
///
/// Support: Full.
pub fn is_blocked(vk: u32, alt: bool, ctrl: bool) -> bool {
    match vk {
        VK_LWIN | VK_RWIN => true,
        VK_TAB | VK_F4 => alt,
        VK_ESCAPE => ctrl,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_chords_are_blocked() {
        assert!(is_blocked(VK_TAB, true, false), "Alt+Tab");
        assert!(is_blocked(VK_LWIN, false, false), "Win");
        assert!(is_blocked(VK_RWIN, false, false), "Right Win");
        assert!(is_blocked(VK_F4, true, false), "Alt+F4");
        assert!(is_blocked(VK_ESCAPE, false, true), "Ctrl+Esc");
    }

    #[test]
    fn the_same_keys_without_their_modifier_pass() {
        assert!(!is_blocked(VK_TAB, false, false), "Tab alone");
        assert!(!is_blocked(VK_F4, false, true), "F4 with Ctrl only");
        assert!(
            !is_blocked(VK_ESCAPE, true, false),
            "Alt+Esc is not in the list"
        );
        assert!(!is_blocked(0x41, true, true), "Ctrl+Alt+A");
    }
}
