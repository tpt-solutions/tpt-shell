// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Kiosk policy values (spec §10.1). A signed kiosk config (see [`crate::config`]) is parsed
//! here into a [`KioskPolicy`]. The platform backends read the policy and apply it: full-screen
//! and single-window placement, the feature-disable list, and the navigation allowlist.
//!
//! The config keys are:
//!
//! - `mode`: `"single-app"` (the default for a kiosk config) or `"browser"`.
//! - `home`: a path under the app origin, default `index.html`.
//! - `idle_seconds`: a whole number above 0. Optional; absent means no idle reset.
//! - `allow`: extra URLs the windows may navigate to. Each entry is an absolute URL or a path
//!   under the app origin. Prefix matches count only at a path boundary.
//! - `fullscreen` and `single_window`: window placement, both default `true` in single-app mode.
//! - `disable`: an object of booleans, see [`Lockdown`]. Unset keys take the mode's default.

use std::fmt;
use std::time::Duration;

use serde_json::Value;

/// Whether the kiosk is locked to one app or behaves like a browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Full-screen, single window, and navigation only inside the allowlist.
    SingleApp,
    /// No placement or navigation limits. Only the idle reset applies.
    Browser,
}

/// Features a kiosk turns off. Each `true` field is a feature that is disabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lockdown {
    pub devtools: bool,
    pub context_menu: bool,
    pub zoom: bool,
    /// Off by default: text selection is optional in the spec, and many kiosks need it.
    pub text_selection: bool,
    pub drag_drop: bool,
    pub print_shortcuts: bool,
    pub function_keys: bool,
    pub swipe_gestures: bool,
    /// Blocks Alt+Tab, the Win key, Alt+F4 and Ctrl+Esc. Best effort: see the Windows hook.
    pub key_chords: bool,
}

impl Lockdown {
    /// Everything a single-app kiosk turns off by default.
    pub fn single_app() -> Self {
        Lockdown {
            devtools: true,
            context_menu: true,
            zoom: true,
            text_selection: false,
            drag_drop: true,
            print_shortcuts: true,
            function_keys: true,
            swipe_gestures: true,
            key_chords: true,
        }
    }

    /// Nothing is turned off.
    pub fn none() -> Self {
        Lockdown {
            devtools: false,
            context_menu: false,
            zoom: false,
            text_selection: false,
            drag_drop: false,
            print_shortcuts: false,
            function_keys: false,
            swipe_gestures: false,
            key_chords: false,
        }
    }
}

/// Window placement the policy asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Windows {
    pub fullscreen: bool,
    pub single_window: bool,
}

/// A parsed kiosk config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KioskPolicy {
    pub mode: Mode,
    /// Path under the app origin that the windows return to.
    pub home: String,
    /// After this long with no input, the windows return home. `None` never resets.
    pub idle: Option<Duration>,
    /// Extra navigation targets, as written in the config.
    pub allow: Vec<String>,
    pub windows: Windows,
    pub lockdown: Lockdown,
}

/// Why a kiosk config was refused. The message names the key.
#[derive(Debug, PartialEq, Eq)]
pub struct PolicyError(pub String);

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PolicyError {}

impl KioskPolicy {
    /// Whether the windows may navigate to `url`. `app_origin` is the origin the app's own pages
    /// are served from. Browser mode allows everything; single-app mode allows the app origin
    /// and the `allow` entries. Anything else, including a non-URL, is refused.
    pub fn allows_navigation(&self, app_origin: &str, url: &str) -> bool {
        if self.mode == Mode::Browser {
            return true;
        }
        if url.contains(['\n', '\r', '\t', ' ']) {
            return false;
        }
        let app = app_origin.trim_end_matches('/');
        if within(url, app) {
            return true;
        }
        self.allow.iter().any(|entry| {
            let target = resolve(entry, app);
            within(url, &target)
        })
    }
}

/// `url` is `prefix` or lies under it at a path boundary (`/`, `?`, or `#` next).
fn within(url: &str, prefix: &str) -> bool {
    if prefix.is_empty() || !url.starts_with(prefix) {
        return false;
    }
    match url[prefix.len()..].chars().next() {
        None => true,
        Some(c) => prefix.ends_with('/') || c == '/' || c == '?' || c == '#',
    }
}

/// An allow entry as an absolute prefix. Paths are taken under the app origin.
fn resolve(entry: &str, app: &str) -> String {
    if entry.contains("://") {
        entry.to_string()
    } else {
        format!("{app}/{}", entry.trim_start_matches('/'))
    }
}

/// Parses a kiosk config. Fails on a bad value, naming the key that is wrong.
pub fn parse(config: &Value) -> Result<KioskPolicy, PolicyError> {
    let mode = match config.get("mode").map(|v| v.as_str()) {
        None => Mode::SingleApp,
        Some(Some("single-app")) => Mode::SingleApp,
        Some(Some("browser")) => Mode::Browser,
        Some(_) => {
            return Err(PolicyError(
                "mode must be \"single-app\" or \"browser\"".into(),
            ))
        }
    };

    let home = match config.get("home") {
        None => "index.html".to_string(),
        Some(value) => value
            .as_str()
            .ok_or_else(|| PolicyError("home must be a path string".into()))?
            .to_string(),
    };
    let plain = !home.is_empty()
        && !home.starts_with('/')
        && !home.contains("..")
        && !home.contains(['\\', ':', '?', '#']);
    if !plain {
        return Err(PolicyError(format!(
            "home {home:?} is not a plain path under the app"
        )));
    }

    let idle = match config.get("idle_seconds") {
        None => None,
        Some(value) => Some(Duration::from_secs(
            value
                .as_u64()
                .filter(|s| *s > 0)
                .ok_or_else(|| PolicyError("idle_seconds must be a whole number above 0".into()))?,
        )),
    };

    let allow = match config.get("allow") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                let text = item
                    .as_str()
                    .ok_or_else(|| PolicyError("allow entries must be strings".into()))?;
                if text.is_empty() || text.contains(['*', '\n', '\r', ' ']) {
                    return Err(PolicyError(format!("{text:?} is not an allowed URL")));
                }
                Ok(text.to_string())
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(PolicyError("allow must be a list of strings".into())),
    };

    let windows = Windows {
        fullscreen: flag(config, "fullscreen", mode == Mode::SingleApp)?,
        single_window: flag(config, "single_window", mode == Mode::SingleApp)?,
    };

    let base = match mode {
        Mode::SingleApp => Lockdown::single_app(),
        Mode::Browser => Lockdown::none(),
    };
    let lockdown = match config.get("disable") {
        None => base,
        Some(Value::Object(map)) => {
            let mut out = base;
            for (key, value) in map {
                let on = value
                    .as_bool()
                    .ok_or_else(|| PolicyError(format!("disable.{key} must be true or false")))?;
                let slot = match key.as_str() {
                    "devtools" => &mut out.devtools,
                    "context_menu" => &mut out.context_menu,
                    "zoom" => &mut out.zoom,
                    "text_selection" => &mut out.text_selection,
                    "drag_drop" => &mut out.drag_drop,
                    "print_shortcuts" => &mut out.print_shortcuts,
                    "function_keys" => &mut out.function_keys,
                    "swipe_gestures" => &mut out.swipe_gestures,
                    "key_chords" => &mut out.key_chords,
                    other => return Err(PolicyError(format!("disable.{other} is not a feature"))),
                };
                *slot = on;
            }
            out
        }
        Some(_) => return Err(PolicyError("disable must be an object".into())),
    };

    Ok(KioskPolicy {
        mode,
        home,
        idle,
        allow,
        windows,
        lockdown,
    })
}

fn flag(config: &Value, key: &str, default: bool) -> Result<bool, PolicyError> {
    match config.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| PolicyError(format!("{key} must be true or false"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ORIGIN: &str = "https://tpt-app.localhost";

    #[test]
    fn a_kiosk_config_defaults_to_single_app_with_everything_locked() {
        let policy = parse(&json!({})).expect("parse");
        assert_eq!(policy.mode, Mode::SingleApp);
        assert_eq!(policy.home, "index.html");
        assert_eq!(policy.idle, None);
        assert!(policy.windows.fullscreen && policy.windows.single_window);
        assert_eq!(policy.lockdown, Lockdown::single_app());
        assert!(!policy.lockdown.text_selection, "selection is left on");
    }

    #[test]
    fn browser_mode_turns_no_feature_off_unless_asked() {
        let policy = parse(&json!({"mode": "browser", "disable": {"zoom": true}})).expect("parse");
        assert_eq!(policy.mode, Mode::Browser);
        assert!(!policy.windows.fullscreen);
        assert!(policy.lockdown.zoom);
        assert!(!policy.lockdown.devtools);
    }

    #[test]
    fn disable_overrides_the_mode_default_per_feature() {
        let policy = parse(&json!({
            "disable": {"text_selection": true, "devtools": false}
        }))
        .expect("parse");
        assert!(policy.lockdown.text_selection);
        assert!(!policy.lockdown.devtools);
    }

    #[test]
    fn bad_values_are_refused_and_the_key_is_named() {
        for (config, key) in [
            (json!({"mode": "kiosk"}), "mode"),
            (json!({"home": "../x.html"}), "home"),
            (json!({"idle_seconds": 0}), "idle_seconds"),
            (json!({"fullscreen": "yes"}), "fullscreen"),
            (json!({"disable": {"reboot": true}}), "reboot"),
            (json!({"disable": {"zoom": 1}}), "disable.zoom"),
            (json!({"allow": ["https://*.example.com"]}), "allowed URL"),
        ] {
            let err = parse(&config).expect_err("refused");
            assert!(err.0.contains(key), "{config} gave {err}");
        }
    }

    #[test]
    fn single_app_navigation_stays_inside_the_allowlist() {
        let policy = parse(&json!({
            "allow": ["help.html", "https://pay.example.com/"]
        }))
        .expect("parse");
        assert!(policy.allows_navigation(ORIGIN, "https://tpt-app.localhost/index.html"));
        assert!(policy.allows_navigation(ORIGIN, "https://tpt-app.localhost/help.html?x=1"));
        assert!(policy.allows_navigation(ORIGIN, "https://pay.example.com/checkout"));
        assert!(!policy.allows_navigation(ORIGIN, "https://evil.example.com/"));
        assert!(!policy.allows_navigation(ORIGIN, "https://tpt-app.localhost.evil.example/"));
        assert!(!policy.allows_navigation(ORIGIN, "https://pay.example.com.evil/x"));
        assert!(!policy.allows_navigation(ORIGIN, "https://tpt-app.localhost/x\ny"));
        assert!(!policy.allows_navigation(ORIGIN, "file:///C:/Windows/system.ini"));
    }

    #[test]
    fn a_path_entry_does_not_match_a_longer_name() {
        let policy = parse(&json!({"allow": ["https://help.example.com/help"]})).expect("parse");
        assert!(policy.allows_navigation(ORIGIN, "https://help.example.com/help"));
        assert!(policy.allows_navigation(ORIGIN, "https://help.example.com/help/more"));
        assert!(!policy.allows_navigation(ORIGIN, "https://help.example.com/helpers"));
    }

    #[test]
    fn browser_mode_allows_any_navigation() {
        let policy = parse(&json!({"mode": "browser"})).expect("parse");
        assert!(policy.allows_navigation(ORIGIN, "https://anywhere.example/"));
    }
}
