// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Drag regions on the GTK backends.
//!
//! WebKitGTK does not implement `-webkit-app-region`, so a region is a set of positioned boxes
//! (`data-tpt-drag`) in the page, as on Windows. A page script listens for a primary-button press
//! on one of those boxes and posts its screen position to the host. The host takes that message
//! before the page's own message handler sees it, and asks GTK to start moving the window from the
//! press (`gtk_window_begin_move_drag` on GTK 3, `gdk_toplevel_begin_move` on GTK 4).
//!
//! Limits: whether a window manager moves the window is up to the desktop. Under a bare X server
//! (such as Xvfb in CI) the move request is made but nothing moves. The request also carries the
//! press position from the page, so it reaches the host a little after the press.

use crate::LogicalRect;

/// The prefix of the message the page sends on a press inside a drag box: `tpt:drag:<x>:<y>`.
pub const MESSAGE_PREFIX: &str = "tpt:drag:";

/// Page script, injected at document start. It posts the press position for a primary-button press
/// on a drag box. Capture phase, so a page that stops the event still starts the drag.
pub const LISTENER_JS: &str = r#"(function () {
  document.addEventListener('mousedown', function (e) {
    if (e.button !== 0 || !e.target || !e.target.closest) { return; }
    if (e.target.closest('[data-tpt-drag]')) {
      window.chrome.webview.postMessage('tpt:drag:' + e.screenX + ':' + e.screenY);
    }
  }, true);
})();
"#;

/// Reads a drag request from a page message. Returns the press position in screen coordinates.
pub fn parse_request(message: &str) -> Option<(f64, f64)> {
    let rest = message.strip_prefix(MESSAGE_PREFIX)?;
    let (x, y) = rest.split_once(':')?;
    let x: f64 = x.parse().ok()?;
    let y: f64 = y.parse().ok()?;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// Script that replaces the page's drag boxes with `regions` (logical units, window coordinates).
/// No regions removes the boxes. Does nothing before the page has a body; the caller sets the
/// regions again after load.
pub fn regions_script(regions: &[LogicalRect]) -> String {
    let mut boxes = String::from("[");
    for (i, r) in regions.iter().enumerate() {
        if i > 0 {
            boxes.push(',');
        }
        boxes.push_str(&format!("[{},{},{},{}]", r.x, r.y, r.width, r.height));
    }
    boxes.push(']');
    format!(
        "(function(){{\
var old=document.querySelectorAll('[data-tpt-drag]');\
for(var i=0;i<old.length;i++){{old[i].remove();}}\
if(!document.body){{return;}}\
{boxes}.forEach(function(r){{\
var d=document.createElement('div');\
d.setAttribute('data-tpt-drag','');\
d.style.position='fixed';d.style.left=r[0]+'px';d.style.top=r[1]+'px';\
d.style.width=r[2]+'px';d.style.height=r[3]+'px';\
d.style.zIndex='2147483646';d.style.background='transparent';\
d.style.setProperty('-webkit-app-region','drag');\
document.body.appendChild(d);\
}});\
}})();"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_message_gives_the_screen_position() {
        assert_eq!(parse_request("tpt:drag:120:45"), Some((120.0, 45.0)));
        assert_eq!(parse_request("tpt:drag:-3.5:7"), Some((-3.5, 7.0)));
    }

    #[test]
    fn other_messages_are_not_drag_requests() {
        assert_eq!(parse_request("ping"), None);
        assert_eq!(parse_request("tpt:drag:x:1"), None);
        assert_eq!(parse_request("tpt:drag:1"), None);
        assert_eq!(parse_request("tpt:drag:NaN:1"), None);
    }

    #[test]
    fn regions_script_lays_out_one_box_per_region() {
        let script = regions_script(&[LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 32.0,
        }]);
        assert!(script.contains("[[0,0,800,32]].forEach"), "{script}");
        assert!(
            script.contains("querySelectorAll('[data-tpt-drag]')"),
            "{script}"
        );
        assert!(regions_script(&[]).contains("[].forEach"));
    }

    #[test]
    fn the_listener_posts_only_primary_presses_on_a_drag_box() {
        assert!(LISTENER_JS.contains("e.button !== 0"));
        assert!(LISTENER_JS.contains("closest('[data-tpt-drag]')"));
        assert!(LISTENER_JS.contains(MESSAGE_PREFIX));
    }
}
