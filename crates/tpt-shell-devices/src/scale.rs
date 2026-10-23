// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Scales (spec §10.2, M7): one generic ASCII line parser. Many serial scales send one line per
//! reading, with a weight, a unit, and a stability word. This parser reads that shape and
//! refuses anything else. Vendor-specific protocols are not implemented.
//!
//! A line is split on spaces and commas. It must hold one number (optionally with a sign and up
//! to three decimals, or with the unit glued on, as in `1.250kg`), one unit (`g`, `kg`, `lb` or
//! `oz`), and only known status words: `ST` or `US` for stable or unstable, and `GS`, `NT` or
//! `TR` for gross, net or tare. Any other word refuses the whole line.
//!
//! Support: Partial. The line shape is generic; a scale that sends a different shape needs its
//! own parser.

use crate::transport::{DeviceError, Transport};

/// A unit a scale reports in.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Gram,
    Kilogram,
    Pound,
    Ounce,
}

/// A weight in thousandths of its unit, so `1.250 kg` is `1250` thousandths of a kilogram.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weight {
    pub thousandths: i64,
    pub unit: Unit,
}

/// One reading from a scale.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub weight: Weight,
    /// True when the scale reports the weight as settled.
    pub stable: bool,
}

/// Why a line was refused.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScaleError {
    /// The line had no words in it.
    Empty,
    /// A word that is not a number, unit or known status word.
    UnknownWord(String),
    /// No weight in the line.
    NoWeight,
    /// No unit in the line.
    NoUnit,
    /// More than one weight, or more than one unit.
    Ambiguous,
    /// A number with more than three decimals, or too large to hold.
    BadNumber(String),
    /// A tagged line with other than three fields.
    WrongFieldCount(usize),
    /// A fixed-width record that is not twelve bytes long.
    BadLength(usize),
}

impl std::fmt::Display for ScaleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScaleError::Empty => write!(f, "scale sent an empty line"),
            ScaleError::UnknownWord(w) => write!(f, "scale sent `{w}`, which is not a known word"),
            ScaleError::NoWeight => write!(f, "scale line has no weight"),
            ScaleError::NoUnit => write!(f, "scale line has no unit"),
            ScaleError::Ambiguous => write!(f, "scale line has more than one weight or unit"),
            ScaleError::BadNumber(n) => write!(f, "`{n}` is not a weight this parser can hold"),
            ScaleError::WrongFieldCount(n) => write!(f, "tagged line has {n} fields, not three"),
            ScaleError::BadLength(n) => write!(f, "fixed-width record is {n} bytes, not twelve"),
        }
    }
}

impl std::error::Error for ScaleError {}

/// Parses one reading line.
///
/// Support: Partial. The line shape is generic; a scale that sends a different shape needs its own
/// parser.
pub fn parse_line(line: &str) -> Result<Reading, ScaleError> {
    let mut weight: Option<i64> = None;
    let mut unit: Option<Unit> = None;
    let mut stable: Option<bool> = None;
    let mut any = false;
    for word in line.split([' ', ',', '\t']).filter(|w| !w.is_empty()) {
        any = true;
        // A number may have its unit glued on: `1.250kg`.
        let split_at = word
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(word.len());
        let (number, glued) = word.split_at(split_at);
        if !number.is_empty() && weight.replace(parse_thousandths(number)?).is_some() {
            return Err(ScaleError::Ambiguous);
        }
        if glued.is_empty() {
            continue;
        }
        match glued.to_ascii_lowercase().as_str() {
            "g" => set_unit(&mut unit, Unit::Gram)?,
            "kg" => set_unit(&mut unit, Unit::Kilogram)?,
            "lb" => set_unit(&mut unit, Unit::Pound)?,
            "oz" => set_unit(&mut unit, Unit::Ounce)?,
            "st" | "stable" => stable = Some(true),
            "us" | "unstable" => stable = Some(false),
            "gs" | "nt" | "tr" => {}
            _ => return Err(ScaleError::UnknownWord(word.to_string())),
        }
    }
    if !any {
        return Err(ScaleError::Empty);
    }
    let thousandths = weight.ok_or(ScaleError::NoWeight)?;
    let unit = unit.ok_or(ScaleError::NoUnit)?;
    Ok(Reading {
        weight: Weight { thousandths, unit },
        // A line without a status word is treated as unstable, the safe reading.
        stable: stable.unwrap_or(false),
    })
}

fn set_unit(slot: &mut Option<Unit>, unit: Unit) -> Result<(), ScaleError> {
    if slot.replace(unit).is_some() {
        return Err(ScaleError::Ambiguous);
    }
    Ok(())
}

/// Parses `-1.250`, `+3`, or `12.5` into thousandths. More than three decimals is refused, not
/// rounded.
fn parse_thousandths(text: &str) -> Result<i64, ScaleError> {
    let bad = || ScaleError::BadNumber(text.to_string());
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (whole, frac) = match digits.split_once('.') {
        Some((w, f)) => (w, f),
        None => (digits, ""),
    };
    let ok = !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && frac.len() <= 3
        && frac.bytes().all(|b| b.is_ascii_digit())
        && (!digits.contains('.') || !frac.is_empty());
    if !ok {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let mut frac_value: i64 = frac.parse().unwrap_or(0);
    for _ in frac.len()..3 {
        frac_value *= 10;
    }
    let magnitude = whole
        .checked_mul(1000)
        .and_then(|w| w.checked_add(frac_value))
        .ok_or_else(bad)?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// Reads whatever the scale has sent and returns the readings it completed. Lines are split on
/// carriage return or line feed. A refused line is returned as an error and does not stop the
/// lines after it.
///
/// Support: Partial. The line shape is generic; a scale that sends a different shape needs its own
/// parser.
pub fn read_readings(
    transport: &mut dyn Transport,
    pending: &mut Vec<u8>,
) -> Result<Vec<Result<Reading, ScaleError>>, DeviceError> {
    pending.extend(transport.read(512)?);
    let mut out = Vec::new();
    while let Some(end) = pending.iter().position(|b| *b == b'\r' || *b == b'\n') {
        let line: Vec<u8> = pending.drain(..=end).collect();
        let text = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();
        if !text.trim().is_empty() {
            out.push(parse_line(&text));
        }
    }
    // Fail closed on a runaway line with no terminator.
    if pending.len() > 256 {
        pending.clear();
        out.push(Err(ScaleError::Empty));
    }
    Ok(out)
}

/// The line shapes this module reads. All three are this crate's own layouts, written down here;
/// none of them is a vendor's protocol.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineFormat {
    /// The generic weight line: a number, a unit and status words in any order, separated by spaces
    /// or commas. See the module docs.
    WeightLine,
    /// Exactly three comma-separated fields: a status (`S` stable or `U` unstable), a decimal value
    /// with an optional sign, and a unit (`g`, `kg`, `lb` or `oz`). For example `S,+1.250,kg`.
    Tagged,
    /// Exactly twelve bytes: a status (`S` or `U`), a sign, a value in the form `DDDD.DDD`, and a
    /// two-byte unit (`kg`, `g `, `lb` or `oz`, the last two padded with a space). For example
    /// `S+0001.250kg`.
    FixedWidth,
}

/// Parses one line in the given format.
///
/// Support: Partial. The three formats are this crate's own. A scale that sends a different layout
/// needs its own parser, and none of these is checked against a vendor's manual.
pub fn parse_as(format: LineFormat, line: &str) -> Result<Reading, ScaleError> {
    match format {
        LineFormat::WeightLine => parse_line(line),
        LineFormat::Tagged => parse_tagged(line),
        LineFormat::FixedWidth => parse_fixed_width(line),
    }
}

/// Parses a [`LineFormat::Tagged`] line.
///
/// Support: Partial. The format is this crate's own; see [`LineFormat::Tagged`].
fn parse_tagged(line: &str) -> Result<Reading, ScaleError> {
    let fields: Vec<&str> = line.trim().split(',').collect();
    let [status, value, unit] = fields.as_slice() else {
        return Err(ScaleError::WrongFieldCount(fields.len()));
    };
    Ok(Reading {
        weight: Weight {
            thousandths: parse_thousandths(value.trim())?,
            unit: parse_unit(unit.trim())?,
        },
        stable: parse_status(status.trim())?,
    })
}

/// Parses a [`LineFormat::FixedWidth`] record.
///
/// Support: Partial. The format is this crate's own; see [`LineFormat::FixedWidth`].
fn parse_fixed_width(line: &str) -> Result<Reading, ScaleError> {
    if !line.is_ascii() || line.len() != 12 {
        return Err(ScaleError::BadLength(line.len()));
    }
    let stable = parse_status(&line[0..1])?;
    // Sign and value: `+DDDD.DDD`, nine bytes with the point at byte 5.
    let value = &line[1..10];
    if value.len() != 9 || value.as_bytes()[5] != b'.' {
        return Err(ScaleError::BadNumber(value.to_string()));
    }
    Ok(Reading {
        weight: Weight {
            thousandths: parse_thousandths(value)?,
            unit: parse_unit(line[10..12].trim_end())?,
        },
        stable,
    })
}

fn parse_status(text: &str) -> Result<bool, ScaleError> {
    match text {
        "S" => Ok(true),
        "U" => Ok(false),
        other => Err(ScaleError::UnknownWord(other.to_string())),
    }
}

fn parse_unit(text: &str) -> Result<Unit, ScaleError> {
    match text.to_ascii_lowercase().as_str() {
        "g" => Ok(Unit::Gram),
        "kg" => Ok(Unit::Kilogram),
        "lb" => Ok(Unit::Pound),
        "oz" => Ok(Unit::Ounce),
        _ => Err(ScaleError::UnknownWord(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weight(thousandths: i64, unit: Unit) -> Weight {
        Weight { thousandths, unit }
    }

    #[test]
    fn a_stable_kilogram_line_parses() {
        let r = parse_line("ST,GS    1.250 kg").expect("parse");
        assert_eq!(r.weight, weight(1250, Unit::Kilogram));
        assert!(r.stable);
    }

    #[test]
    fn a_glued_unit_and_a_negative_value_parse() {
        let r = parse_line("US -0.5g").expect("parse");
        assert_eq!(r.weight, weight(-500, Unit::Gram));
        assert!(!r.stable);
        assert_eq!(
            parse_line("12lb").expect("parse").weight,
            weight(12000, Unit::Pound)
        );
    }

    #[test]
    fn a_line_without_a_status_word_is_unstable() {
        assert!(!parse_line("3 oz").expect("parse").stable);
    }

    #[test]
    fn bad_lines_are_refused_with_a_reason() {
        assert_eq!(parse_line("   "), Err(ScaleError::Empty));
        assert_eq!(parse_line("ST kg"), Err(ScaleError::NoWeight));
        assert_eq!(parse_line("1.250"), Err(ScaleError::NoUnit));
        assert_eq!(parse_line("1 2 kg"), Err(ScaleError::Ambiguous));
        assert_eq!(parse_line("1 kg g"), Err(ScaleError::Ambiguous));
        assert!(matches!(
            parse_line("1 kg ZZ"),
            Err(ScaleError::UnknownWord(_))
        ));
        assert!(matches!(
            parse_line("1.2345 kg"),
            Err(ScaleError::BadNumber(_))
        ));
        assert!(matches!(parse_line("1. kg"), Err(ScaleError::BadNumber(_))));
        assert!(matches!(
            parse_line("9999999999999999999 kg"),
            Err(ScaleError::BadNumber(_))
        ));
        assert!(matches!(
            parse_line("1e3 kg"),
            Err(ScaleError::UnknownWord(_))
        ));
    }

    #[test]
    fn lines_are_read_from_a_transport_and_bad_ones_do_not_stop_the_rest() {
        use crate::id::DeviceId;
        use crate::virtual_devices::VirtualSerial;
        let id: DeviceId = "serial:COM4".parse().expect("id");
        let mut port = VirtualSerial::loopback(id);
        port.write(b"ST 1.000 kg\r\nbogus\r\nUS 0.998 kg\r\n")
            .expect("write");
        let mut pending = Vec::new();
        let got = read_readings(&mut port, &mut pending).expect("read");
        assert_eq!(got.len(), 3);
        assert!(got[0].is_ok());
        assert!(got[1].is_err());
        assert_eq!(got[2].as_ref().expect("third").weight.thousandths, 998);
        assert!(pending.is_empty());
    }

    #[test]
    fn a_tagged_record_parses_with_a_sign_and_a_status() {
        let r = parse_as(LineFormat::Tagged, "S,+1.250,kg").expect("parse");
        assert_eq!(r.weight, weight(1250, Unit::Kilogram));
        assert!(r.stable);
        let r = parse_as(LineFormat::Tagged, "U,-0.5,g").expect("parse");
        assert_eq!(r.weight, weight(-500, Unit::Gram));
        assert!(!r.stable);
    }

    #[test]
    fn a_tagged_record_refuses_the_wrong_shape() {
        assert_eq!(
            parse_as(LineFormat::Tagged, "S,1.250"),
            Err(ScaleError::WrongFieldCount(2))
        );
        assert_eq!(
            parse_as(LineFormat::Tagged, "X,1.250,kg"),
            Err(ScaleError::UnknownWord("X".to_string()))
        );
        assert_eq!(
            parse_as(LineFormat::Tagged, "S,1.250,stone"),
            Err(ScaleError::UnknownWord("stone".to_string()))
        );
    }

    #[test]
    fn a_fixed_width_record_parses_at_exactly_twelve_bytes() {
        let r = parse_as(LineFormat::FixedWidth, "S+0001.250kg").expect("parse");
        assert_eq!(r.weight, weight(1250, Unit::Kilogram));
        assert!(r.stable);
        let r = parse_as(LineFormat::FixedWidth, "U-0000.500g ").expect("parse");
        assert_eq!(r.weight, weight(-500, Unit::Gram));
        assert!(!r.stable);
    }

    #[test]
    fn a_fixed_width_record_refuses_a_wrong_length_or_point() {
        assert_eq!(
            parse_as(LineFormat::FixedWidth, "S+001.250kg"),
            Err(ScaleError::BadLength(11))
        );
        assert_eq!(
            parse_as(LineFormat::FixedWidth, "S+00012.50kg"),
            Err(ScaleError::BadNumber("+00012.50".to_string()))
        );
        assert_eq!(
            parse_as(LineFormat::FixedWidth, "S+0001.250 g"),
            Err(ScaleError::UnknownWord(" g".to_string()))
        );
    }

    #[test]
    fn the_weight_line_format_is_the_default_parser() {
        assert_eq!(
            parse_as(LineFormat::WeightLine, "ST 1.000 kg"),
            parse_line("ST 1.000 kg")
        );
    }
}
