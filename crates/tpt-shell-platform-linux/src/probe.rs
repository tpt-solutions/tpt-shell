// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Which GTK and WebKit adapter this machine can run, for `doctor` (spec §6.3, todo M5).
//!
//! Selection is by which libraries load: the GTK 3 and WebKit2GTK 4.1 adapter is picked when its
//! libraries open. GTK 4 and WebKitGTK 6.0 are reported, not selected, because that adapter is
//! not written yet.

use crate::gtk3;
use crate::loader::SharedLibrary;

/// An adapter this crate can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// GTK 3 with WebKit2GTK 4.1.
    Gtk3,
}

/// The display the session offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    X11,
    Wayland,
    /// Both variables are set: a Wayland session with XWayland, or a nested setup. GTK picks.
    Both,
    None,
}

/// What was found on this machine.
#[derive(Debug)]
pub struct Probe {
    /// `Ok` with the library names that opened, or the reason it did not.
    pub gtk3: Result<String, String>,
    /// The GTK 4 adapter is not written, so this is only whether its libraries open.
    pub gtk4_libraries: Result<String, String>,
    pub display: Display,
    /// The adapter that would run, if any.
    pub selected: Option<Adapter>,
}

/// Probes the libraries and the display. Opens libraries but calls no GTK function, so it does
/// not start GTK or need a display.
pub fn probe() -> Probe {
    let gtk3_result = match gtk3::sys::load() {
        Ok(_) => Ok("GTK 3 and WebKit2GTK 4.1 loaded".to_string()),
        Err(e) => Err(e.to_string()),
    };
    let gtk4_libraries = match (
        SharedLibrary::open(&["libgtk-4.so.1"]),
        SharedLibrary::open(&["libwebkitgtk-6.0.so.4"]),
    ) {
        (Ok(_), Ok(_)) => {
            Ok("GTK 4 and WebKitGTK 6.0 libraries are present (adapter not built)".to_string())
        }
        (Err(e), _) | (_, Err(e)) => Err(e.to_string()),
    };
    let selected = gtk3_result.as_ref().ok().map(|_| Adapter::Gtk3);
    Probe {
        gtk3: gtk3_result,
        gtk4_libraries,
        display: display(),
        selected,
    }
}

/// The display from the environment. Pure apart from reading variables.
pub fn display() -> Display {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    match (set("DISPLAY"), set("WAYLAND_DISPLAY")) {
        (true, true) => Display::Both,
        (true, false) => Display::X11,
        (false, true) => Display::Wayland,
        (false, false) => Display::None,
    }
}

impl Probe {
    /// One line per fact, for the `doctor` output.
    pub fn report(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(match &self.gtk3 {
            Ok(text) => format!("gtk3: ok ({text})"),
            Err(text) => format!("gtk3: missing ({text})"),
        });
        lines.push(match &self.gtk4_libraries {
            Ok(text) => format!("gtk4: {text}"),
            Err(text) => format!("gtk4: not available ({text})"),
        });
        lines.push(format!(
            "display: {}",
            match self.display {
                Display::X11 => "X11",
                Display::Wayland => "Wayland",
                Display::Both => "X11 and Wayland",
                Display::None => "none (no DISPLAY or WAYLAND_DISPLAY)",
            }
        ));
        lines.push(match self.selected {
            Some(Adapter::Gtk3) => "adapter: GTK 3 with WebKit2GTK 4.1".to_string(),
            None => "adapter: none (install libgtk-3-0 and libwebkit2gtk-4.1-0)".to_string(),
        });
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_has_a_line_for_each_fact() {
        let probe = Probe {
            gtk3: Err("libgtk-3.so.0 missing".into()),
            gtk4_libraries: Err("libgtk-4.so.1 missing".into()),
            display: Display::None,
            selected: None,
        };
        let lines = probe.report();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("gtk3: missing"));
        assert!(lines[3].contains("adapter: none"));
    }

    #[test]
    fn probing_never_panics_and_selects_only_a_loaded_adapter() {
        let p = probe();
        assert_eq!(p.selected.is_some(), p.gtk3.is_ok());
        assert!(!p.report().is_empty());
    }
}
