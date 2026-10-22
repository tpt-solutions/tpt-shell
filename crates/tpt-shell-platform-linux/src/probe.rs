// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Which GTK and WebKit adapter this machine can run, for `doctor` (spec §6.3, todo M5).
//!
//! Selection is by which libraries load. When both adapters load, GTK 4 with WebKitGTK 6.0 is
//! preferred, because GTK 3 is in maintenance. The `TPT_GTK` environment variable overrides this:
//! `TPT_GTK=3` or `TPT_GTK=4` asks for one adapter, and if that adapter cannot load the result is
//! an error naming it, not a silent fall back to the other one (a user who asked for GTK 3 to
//! work around a GTK 4 bug must not be handed GTK 4).
//!
//! A process must only run one of the two toolkits. Probing opens the libraries of both, which
//! calls no GTK function and starts neither toolkit.

use std::fmt;

use crate::{gtk3, gtk4};

/// The environment variable that forces an adapter.
///
/// Support: Full.
pub const ENV_OVERRIDE: &str = "TPT_GTK";

/// An adapter this crate can run.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    /// GTK 3 with WebKit2GTK 4.1.
    Gtk3,
    /// GTK 4 with WebKitGTK 6.0.
    Gtk4,
}

impl fmt::Display for Adapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Adapter::Gtk3 => "GTK 3 with WebKit2GTK 4.1",
            Adapter::Gtk4 => "GTK 4 with WebKitGTK 6.0",
        })
    }
}

/// Which adapter the caller asked for.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preference {
    /// Prefer GTK 4, then GTK 3.
    Auto,
    Gtk3,
    Gtk4,
}

/// Reads a preference from the text of `TPT_GTK`. Unset or empty is `Auto`. Anything other than
/// `3`, `4`, `gtk3` or `gtk4` is an error, so a typo is not mistaken for `Auto`.
///
/// Support: Full.
pub fn parse_preference(value: Option<&str>) -> Result<Preference, String> {
    match value.map(str::trim) {
        None | Some("") => Ok(Preference::Auto),
        Some("3") | Some("gtk3") => Ok(Preference::Gtk3),
        Some("4") | Some("gtk4") => Ok(Preference::Gtk4),
        Some(other) => Err(format!(
            "{ENV_OVERRIDE} must be 3 or 4, not `{other}`; unset it to let tpt-shell choose"
        )),
    }
}

/// The preference from the environment.
///
/// Support: Full.
pub fn preference_from_env() -> Result<Preference, String> {
    let value = std::env::var(ENV_OVERRIDE).ok();
    parse_preference(value.as_deref())
}

/// Chooses an adapter. `load3` and `load4` say whether each adapter's libraries load, `Ok` with a
/// description or `Err` with the reason; they are called only when the preference needs them, so
/// `TPT_GTK=3` never opens the GTK 4 libraries. Pure apart from those calls.
///
/// Support: Full.
pub fn select(
    preference: Preference,
    load3: impl FnOnce() -> Result<String, String>,
    load4: impl FnOnce() -> Result<String, String>,
) -> Result<Adapter, String> {
    match preference {
        Preference::Gtk3 => load3().map(|_| Adapter::Gtk3).map_err(|e| {
            format!(
                "{ENV_OVERRIDE}=3 asked for GTK 3, which cannot load: {e}. {}",
                INSTALL_HINT_3
            )
        }),
        Preference::Gtk4 => load4().map(|_| Adapter::Gtk4).map_err(|e| {
            format!(
                "{ENV_OVERRIDE}=4 asked for GTK 4, which cannot load: {e}. {}",
                INSTALL_HINT_4
            )
        }),
        Preference::Auto => match load4() {
            Ok(_) => Ok(Adapter::Gtk4),
            Err(e4) => match load3() {
                Ok(_) => Ok(Adapter::Gtk3),
                Err(e3) => Err(format!(
                    "no Linux window adapter can load. GTK 4 with WebKitGTK 6.0: {e4}. \
                     GTK 3 with WebKit2GTK 4.1: {e3}. {INSTALL_HINT_4}, or {}",
                    INSTALL_HINT_3.to_lowercase()
                )),
            },
        },
    }
}

const INSTALL_HINT_3: &str = "Install libgtk-3-0 and libwebkit2gtk-4.1-0";
const INSTALL_HINT_4: &str = "Install libgtk-4-1 and libwebkitgtk-6.0-4";

/// Tries the GTK 3 libraries and symbols.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
pub fn load_gtk3() -> Result<String, String> {
    match gtk3::sys::load() {
        Ok(_) => Ok("GTK 3 and WebKit2GTK 4.1 loaded".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Tries the GTK 4 libraries and symbols. Notes whether `GtkFileDialog` (GTK 4.10) is there.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
pub fn load_gtk4() -> Result<String, String> {
    match gtk4::sys::load() {
        Ok(gtk) => Ok(if gtk.dialog_fns().is_ok() {
            "GTK 4 and WebKitGTK 6.0 loaded".to_string()
        } else {
            "GTK 4 and WebKitGTK 6.0 loaded; GTK is older than 4.10, so file dialogs are \
             unsupported"
                .to_string()
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// Chooses the adapter the environment asks for, loading only the libraries that needs.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
pub fn select_from_env() -> Result<Adapter, String> {
    select(preference_from_env()?, load_gtk3, load_gtk4)
}

/// The display the session offers.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    X11,
    Wayland,
    /// Both variables are set: a Wayland session with XWayland, or a nested setup. GTK picks.
    Both,
    None,
}

/// What was found on this machine.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
#[derive(Debug)]
pub struct Probe {
    /// `Ok` with a description, or the reason GTK 3 and WebKit2GTK 4.1 did not load.
    pub gtk3: Result<String, String>,
    /// `Ok` with a description, or the reason GTK 4 and WebKitGTK 6.0 did not load.
    pub gtk4: Result<String, String>,
    pub display: Display,
    /// The adapter that would run (honouring `TPT_GTK`), or why none can.
    pub selected: Result<Adapter, String>,
}

/// Probes the libraries and the display. Opens libraries but calls no GTK function, so it does
/// not start GTK or need a display.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
pub fn probe() -> Probe {
    let gtk3_result = load_gtk3();
    let gtk4_result = load_gtk4();
    let selected = match preference_from_env() {
        Ok(preference) => select(preference, || gtk3_result.clone(), || gtk4_result.clone()),
        Err(e) => Err(e),
    };
    Probe {
        gtk3: gtk3_result,
        gtk4: gtk4_result,
        display: display(),
        selected,
    }
}

/// The display from the environment. Pure apart from reading variables.
///
/// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here; compile-
/// checked.
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
    /// True when an adapter can run. A missing display does not change this: `doctor` may run
    /// over SSH.
    ///
    /// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn ok(&self) -> bool {
        self.selected.is_ok()
    }

    /// One line per fact, for the `doctor` output.
    ///
    /// Support: Partial. Loads system libraries at run time. Not run on macOS/Linux here;
    /// compile-checked.
    pub fn report(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(match &self.gtk3 {
            Ok(text) => format!("gtk3: ok ({text})"),
            Err(text) => format!("gtk3: missing ({text})"),
        });
        lines.push(match &self.gtk4 {
            Ok(text) => format!("gtk4: ok ({text})"),
            Err(text) => format!("gtk4: missing ({text})"),
        });
        lines.push(format!(
            "display: {}",
            match self.display {
                Display::X11 => "X11",
                Display::Wayland => "Wayland (no global hotkeys, no window placement)",
                Display::Both => "X11 and Wayland (GTK chooses)",
                Display::None => "none (no DISPLAY or WAYLAND_DISPLAY)",
            }
        ));
        lines.push(match &self.selected {
            Ok(adapter) => format!("adapter: {adapter}"),
            Err(text) => format!("adapter: none ({text})"),
        });
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> Result<String, String> {
        Ok("loaded".into())
    }

    fn missing() -> Result<String, String> {
        Err("lib missing".into())
    }

    #[test]
    fn the_override_is_parsed_strictly() {
        assert_eq!(parse_preference(None), Ok(Preference::Auto));
        assert_eq!(parse_preference(Some("")), Ok(Preference::Auto));
        assert_eq!(parse_preference(Some("3")), Ok(Preference::Gtk3));
        assert_eq!(parse_preference(Some("gtk4")), Ok(Preference::Gtk4));
        assert_eq!(parse_preference(Some(" 4 ")), Ok(Preference::Gtk4));
        assert!(parse_preference(Some("5")).is_err());
        assert!(parse_preference(Some("gtk")).is_err());
    }

    #[test]
    fn auto_prefers_gtk4_when_both_load() {
        assert_eq!(select(Preference::Auto, ok, ok), Ok(Adapter::Gtk4));
    }

    #[test]
    fn auto_falls_back_to_gtk3_only_when_gtk4_will_not_load() {
        assert_eq!(select(Preference::Auto, ok, missing), Ok(Adapter::Gtk3));
        assert_eq!(select(Preference::Auto, missing, ok), Ok(Adapter::Gtk4));
    }

    #[test]
    fn neither_installed_is_a_clear_error_naming_both_and_the_packages() {
        let err = select(Preference::Auto, missing, missing).unwrap_err();
        assert!(err.contains("GTK 4"), "{err}");
        assert!(err.contains("GTK 3"), "{err}");
        assert!(err.contains("libgtk-4-1"), "{err}");
        assert!(err.contains("libwebkit2gtk-4.1-0"), "{err}");
    }

    #[test]
    fn an_explicit_choice_never_falls_back() {
        assert_eq!(select(Preference::Gtk3, ok, ok), Ok(Adapter::Gtk3));
        assert_eq!(select(Preference::Gtk4, ok, ok), Ok(Adapter::Gtk4));
        let err = select(Preference::Gtk3, missing, ok).unwrap_err();
        assert!(err.contains("TPT_GTK=3"), "{err}");
        let err = select(Preference::Gtk4, ok, missing).unwrap_err();
        assert!(err.contains("TPT_GTK=4"), "{err}");
    }

    #[test]
    fn an_explicit_choice_does_not_open_the_other_toolkit() {
        let opened4 = std::cell::Cell::new(false);
        let _ = select(Preference::Gtk3, ok, || {
            opened4.set(true);
            ok()
        });
        assert!(
            !opened4.get(),
            "TPT_GTK=3 must not load the GTK 4 libraries"
        );
    }

    #[test]
    fn the_report_has_a_line_for_each_fact() {
        let probe = Probe {
            gtk3: Err("libgtk-3.so.0 missing".into()),
            gtk4: Err("libgtk-4.so.1 missing".into()),
            display: Display::None,
            selected: Err("nothing loads".into()),
        };
        let lines = probe.report();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("gtk3: missing"));
        assert!(lines[1].starts_with("gtk4: missing"));
        assert!(lines[3].contains("adapter: none"));
        assert!(!probe.ok());
    }

    #[test]
    fn probing_never_panics_and_agrees_with_what_loaded() {
        let p = probe();
        if preference_from_env() == Ok(Preference::Auto) {
            assert_eq!(p.selected.is_ok(), p.gtk3.is_ok() || p.gtk4.is_ok());
            if p.gtk4.is_ok() {
                assert_eq!(p.selected, Ok(Adapter::Gtk4));
            }
        }
        assert!(!p.report().is_empty());
    }
}
