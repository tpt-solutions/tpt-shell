// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! `tpt-shell audit explain`: a plain-language summary of an audit log, for someone who reviews
//! it without reading JSON. The caller runs `audit verify` first, so the text is known good.

use std::collections::BTreeMap;

/// The summary lines for the audit log in `text` (one JSON record per line).
pub fn explain(text: &str) -> Result<Vec<String>, String> {
    let mut times = Vec::new();
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut commands: BTreeMap<String, usize> = BTreeMap::new();
    let mut actors: BTreeMap<String, usize> = BTreeMap::new();
    let mut notable = Vec::new();
    let mut calls = 0usize;
    for (index, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let record: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("record {}: {e}", index + 1))?;
        let seq = field(&record, "seq", index)?;
        let at = record
            .get("wall_unix_ms")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("record {}: no wall_unix_ms", index + 1))?;
        let actor = field(&record, "actor", index)?;
        let command = field(&record, "command", index)?;
        let outcome = field(&record, "outcome", index)?;
        calls += 1;
        times.push(at);
        *outcomes.entry(outcome.clone()).or_default() += 1;
        *commands.entry(command.clone()).or_default() += 1;
        *actors.entry(actor.clone()).or_default() += 1;
        if outcome != "ok" {
            notable.push(format!(
                "  #{seq} at {}: {actor} called {command}; the outcome was {outcome}.",
                utc(at)
            ));
        }
    }
    let mut lines = Vec::new();
    match (times.iter().min(), times.iter().max()) {
        (Some(&first), Some(&last)) => lines.push(format!(
            "The log holds {calls} call(s), from {} to {}.",
            utc(first),
            utc(last)
        )),
        _ => return Ok(vec!["The log holds no calls.".to_string()]),
    }
    lines.push(format!("Outcomes: {}.", counts(&outcomes)));
    lines.push(format!("Commands: {}.", counts(&commands)));
    lines.push(format!("Callers: {}.", counts(&actors)));
    if notable.is_empty() {
        lines.push("Every call succeeded.".to_string());
    } else {
        lines.push(format!("{} call(s) did not succeed:", notable.len()));
        lines.extend(notable);
    }
    Ok(lines)
}

/// A field as text. Numbers and other values are written as JSON.
fn field(record: &serde_json::Value, name: &str, index: usize) -> Result<String, String> {
    match record.get(name) {
        Some(serde_json::Value::String(s)) => Ok(s.clone()),
        Some(other) => Ok(other.to_string()),
        None => Err(format!("record {}: no {name}", index + 1)),
    }
}

/// `name 3, other 1`, most used first, ties by name.
fn counts(map: &BTreeMap<String, usize>) -> String {
    let mut pairs: Vec<(&String, &usize)> = map.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    pairs
        .iter()
        .map(|(name, n)| format!("{name} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `YYYY-MM-DD HH:MM:SS UTC` for a Unix time in milliseconds.
fn utc(unix_ms: u64) -> String {
    let secs = (unix_ms / 1000) as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The civil date of a day count from 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = concat!(
        r#"{"seq":1,"wall_unix_ms":1000000000000,"actor":"page","command":"greet","outcome":"ok"}"#,
        "\n",
        r#"{"seq":2,"wall_unix_ms":1000000060000,"actor":"page","command":"save","outcome":"denied"}"#,
        "\n",
        r#"{"seq":3,"wall_unix_ms":1000000120000,"actor":"agent","command":"greet","outcome":"ok"}"#,
        "\n"
    );

    #[test]
    fn the_summary_counts_and_names_the_failures() {
        let lines = explain(LOG).unwrap();
        assert_eq!(
            lines[0],
            "The log holds 3 call(s), from 2001-09-09 01:46:40 UTC to 2001-09-09 01:48:40 UTC."
        );
        assert_eq!(lines[1], "Outcomes: ok 2, denied 1.");
        assert_eq!(lines[2], "Commands: greet 2, save 1.");
        assert_eq!(lines[3], "Callers: page 2, agent 1.");
        assert_eq!(lines[4], "1 call(s) did not succeed:");
        assert_eq!(
            lines[5],
            "  #2 at 2001-09-09 01:47:40 UTC: page called save; the outcome was denied."
        );
    }

    #[test]
    fn an_empty_log_says_so() {
        assert_eq!(explain("").unwrap(), ["The log holds no calls."]);
    }

    #[test]
    fn a_record_without_a_field_is_refused_with_its_number() {
        let err =
            explain(r#"{"seq":1,"wall_unix_ms":5,"actor":"page","command":"x"}"#).unwrap_err();
        assert!(err.contains("record 1") && err.contains("outcome"), "{err}");
    }

    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_782_400_000), "2000-02-29 00:00:00 UTC");
    }
}
