// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Theme injection (spec M3): CSS custom properties and the colour scheme, set on the page
//! at document start.
//!
//! The values are checked before they reach the page. Colours must be `#rgb` or `#rrggbb`,
//! and the font stack may use only letters, digits, spaces, commas, hyphens and quoted
//! family names, so a theme cannot inject script or break out of the style value.

use crate::error::{Error, Result};
use crate::platform::ColorScheme;

/// Colours and font for the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Theme {
    pub accent: String,
    pub background: String,
    pub foreground: String,
    pub font: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: "#0a64ad".into(),
            background: "#ffffff".into(),
            foreground: "#1b1b1b".into(),
            font: "system-ui, sans-serif".into(),
        }
    }
}

/// Accessibility signals the page can react to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub reduced_transparency: bool,
}

impl Theme {
    /// Checks every value. Returns the first problem as an error.
    pub fn validate(&self) -> Result<()> {
        check_color("accent", &self.accent)?;
        check_color("background", &self.background)?;
        check_color("foreground", &self.foreground)?;
        check_font(&self.font)
    }
}

fn check_color(field: &str, value: &str) -> Result<()> {
    let hex = value.strip_prefix('#');
    let ok = match hex {
        Some(h) => matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit()),
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Error::Backend(format!(
            "theme {field} `{value}` must be #rgb or #rrggbb"
        )))
    }
}

fn check_font(value: &str) -> Result<()> {
    let ok = !value.trim().is_empty()
        && value.len() <= 200
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | ',' | '-' | '"' | '\'' | '_'));
    if ok {
        Ok(())
    } else {
        Err(Error::Backend(format!(
            "theme font `{value}` may contain only letters, digits, spaces, commas, hyphens and quotes"
        )))
    }
}

/// The script to run at document start. It is safe to build once and reuse, and it only sets
/// properties on `document.documentElement`.
///
/// Variables set: `--tpt-accent`, `--tpt-bg`, `--tpt-fg`, `--tpt-font`. The page also gets
/// `data-color-scheme`, `data-high-contrast`, `data-reduced-motion` and
/// `data-reduced-transparency` attributes, and `color-scheme` is set to match.
pub fn injection_script(theme: &Theme, scheme: ColorScheme, signals: Signals) -> Result<String> {
    theme.validate()?;
    let scheme = match scheme {
        ColorScheme::Light => "light",
        ColorScheme::Dark => "dark",
    };
    // At document start the root element may not exist yet. In that case the script waits
    // for DOMContentLoaded, which still runs before the first paint of the page.
    Ok(format!(
        concat!(
            "(function () {{\n",
            "  function apply() {{\n",
            "    var root = document.documentElement;\n",
            "    root.style.setProperty('--tpt-accent', '{}');\n",
            "    root.style.setProperty('--tpt-bg', '{}');\n",
            "    root.style.setProperty('--tpt-fg', '{}');\n",
            "    root.style.setProperty('--tpt-font', '{}');\n",
            "    root.style.setProperty('color-scheme', '{}');\n",
            "    root.dataset.colorScheme = '{}';\n",
            "    root.dataset.highContrast = '{}';\n",
            "    root.dataset.reducedMotion = '{}';\n",
            "    root.dataset.reducedTransparency = '{}';\n",
            "  }}\n",
            "  if (document.documentElement) {{ apply(); }}\n",
            "  else {{ document.addEventListener('DOMContentLoaded', apply); }}\n",
            "}})();\n"
        ),
        theme.accent,
        theme.background,
        theme.foreground,
        theme.font,
        scheme,
        scheme,
        signals.high_contrast,
        signals.reduced_motion,
        signals.reduced_transparency,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_valid() {
        assert_eq!(Theme::default().validate(), Ok(()));
    }

    #[test]
    fn colours_must_be_short_or_long_hex() {
        for good in ["#fff", "#0A64AD"] {
            let t = Theme {
                accent: good.into(),
                ..Theme::default()
            };
            assert!(t.validate().is_ok(), "{good}");
        }
        for bad in ["red", "#ffff", "#gggggg", "#", "#0a64ad;x", "url(x)"] {
            let t = Theme {
                accent: bad.into(),
                ..Theme::default()
            };
            assert!(t.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn font_stacks_cannot_break_out_of_the_style_value() {
        let good = Theme {
            font: "\"Segoe UI\", sans-serif".into(),
            ..Theme::default()
        };
        assert!(good.validate().is_ok());
        for bad in ["x'); alert(1); ('", "a;b", "{}", "", "a\\b"] {
            let t = Theme {
                font: bad.into(),
                ..Theme::default()
            };
            assert!(t.validate().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn script_sets_variables_scheme_and_signals() {
        let script = injection_script(
            &Theme::default(),
            ColorScheme::Dark,
            Signals {
                high_contrast: true,
                ..Signals::default()
            },
        )
        .unwrap();
        assert!(script.contains("setProperty('--tpt-accent', '#0a64ad')"));
        assert!(script.contains("setProperty('color-scheme', 'dark')"));
        assert!(script.contains("root.dataset.highContrast = 'true'"));
        assert!(script.contains("root.dataset.reducedMotion = 'false'"));
    }

    #[test]
    fn invalid_theme_produces_no_script() {
        let t = Theme {
            accent: "nope".into(),
            ..Theme::default()
        };
        assert!(injection_script(&t, ColorScheme::Light, Signals::default()).is_err());
    }

    #[test]
    fn golden_script() {
        let script =
            injection_script(&Theme::default(), ColorScheme::Light, Signals::default()).unwrap();
        assert_eq!(
            script,
            "(function () {\n  function apply() {\n    var root = document.documentElement;\n    root.style.setProperty('--tpt-accent', '#0a64ad');\n    root.style.setProperty('--tpt-bg', '#ffffff');\n    root.style.setProperty('--tpt-fg', '#1b1b1b');\n    root.style.setProperty('--tpt-font', 'system-ui, sans-serif');\n    root.style.setProperty('color-scheme', 'light');\n    root.dataset.colorScheme = 'light';\n    root.dataset.highContrast = 'false';\n    root.dataset.reducedMotion = 'false';\n    root.dataset.reducedTransparency = 'false';\n  }\n  if (document.documentElement) { apply(); }\n  else { document.addEventListener('DOMContentLoaded', apply); }\n})();\n"
        );
    }
}
