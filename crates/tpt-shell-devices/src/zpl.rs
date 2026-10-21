// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! ZPL II label commands. A [`Label`] renders to one `^XA ... ^XZ` block.

use crate::{check_ascii, UnsupportedText};

/// Builds one ZPL label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    body: String,
}

impl Default for Label {
    fn default() -> Self {
        Self::new()
    }
}

impl Label {
    pub fn new() -> Self {
        Label {
            body: String::new(),
        }
    }

    /// Prints `text` at (`x`, `y`) dots, in a font `height` dots high.
    ///
    /// `^` and `~` are ZPL command characters, so they are written as hex escapes with `^FH`.
    pub fn text(
        &mut self,
        x: u32,
        y: u32,
        height: u32,
        text: &str,
    ) -> Result<&mut Self, UnsupportedText> {
        check_ascii(text)?;
        let escaped = escape_field(text);
        let fh = if escaped.contains('_') { "^FH" } else { "" };
        self.body.push_str(&format!(
            "^FO{x},{y}^A0N,{height},{height}{fh}^FD{escaped}^FS\n"
        ));
        Ok(self)
    }

    /// Prints a Code 128 barcode of `data` at (`x`, `y`), `height` dots high.
    pub fn code128(
        &mut self,
        x: u32,
        y: u32,
        height: u32,
        data: &str,
    ) -> Result<&mut Self, UnsupportedText> {
        check_ascii(data)?;
        let escaped = escape_field(data);
        let fh = if escaped.contains('_') { "^FH" } else { "" };
        self.body.push_str(&format!(
            "^FO{x},{y}^BCN,{height},Y,N,N{fh}^FD{escaped}^FS\n"
        ));
        Ok(self)
    }

    /// Renders the complete label, from `^XA` to `^XZ`.
    pub fn render(&self) -> String {
        format!("^XA\n{}^XZ\n", self.body)
    }
}

/// Escapes ZPL's command characters as `_hh` hex, which `^FH` decodes.
fn escape_field(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '^' => out.push_str("_5E"),
            '~' => out.push_str("_7E"),
            '_' => out.push_str("_5F"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_renders_the_expected_commands() {
        let mut label = Label::new();
        label.text(50, 40, 30, "Tea 2.50").unwrap();
        label.code128(50, 100, 60, "12345").unwrap();
        assert_eq!(
            label.render(),
            "^XA\n^FO50,40^A0N,30,30^FDTea 2.50^FS\n^FO50,100^BCN,60,Y,N,N^FD12345^FS\n^XZ\n"
        );
    }

    #[test]
    fn command_characters_in_data_are_escaped() {
        let mut label = Label::new();
        label.text(0, 0, 20, "a^b~c").unwrap();
        let out = label.render();
        assert!(out.contains("^FH^FDa_5Eb_7Ec^FS"), "{out}");
    }

    #[test]
    fn underscores_are_escaped_too_so_they_do_not_read_as_escapes() {
        let mut label = Label::new();
        label.text(0, 0, 20, "a_5E").unwrap();
        assert!(label.render().contains("^FH^FDa_5F5E^FS"));
    }

    #[test]
    fn non_ascii_text_is_refused() {
        assert!(Label::new().text(0, 0, 20, "naïve").is_err());
    }
}
