// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Fuzz targets for the parsers and checkers of tpt-shell (spec §5.5, §8, §10.3).
//!
//! Each target is a plain function that takes arbitrary bytes and must never panic. The
//! `main.rs` driver runs them on mutated inputs for a set time. The tests at the bottom run the
//! same functions over a fixed, seeded set of inputs, so the targets are exercised on every
//! `cargo test --manifest-path fuzz/Cargo.toml`, with no nightly toolchain or fuzzing engine.

use std::sync::atomic::{AtomicU64, Ordering};

use tpt_shell_ipc::schema::{validate, Field, Schema};
use tpt_shell_ipc::Limits;

/// The names of the targets, in the order the driver lists them.
pub const TARGETS: [&str; 5] = ["envelope", "validator", "manifest", "audit_chain", "replay"];

/// Runs the target called `name` on `data`. Returns `false` for an unknown name.
pub fn run_target(name: &str, data: &[u8]) -> bool {
    match name {
        "envelope" => envelope(data),
        "validator" => validator(data),
        "manifest" => manifest(data),
        "audit_chain" => audit_chain(data),
        "replay" => replay(data),
        _ => return false,
    }
    true
}

/// Replay reader: any text, read strictly and leniently. A recording that cannot be read is an
/// error, never a panic.
pub fn replay(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = tpt_shell_replay::read(text, tpt_shell_replay::Mode::Strict);
        let _ = tpt_shell_replay::read(text, tpt_shell_replay::Mode::Lenient);
    }
}

/// Envelope parser: any bytes, as one JS -> Rust message.
pub fn envelope(data: &[u8]) {
    let _ = tpt_shell_ipc::parse_request(data, &Limits::default());
    // A tight limit exercises the size and depth refusals too.
    let tight = Limits {
        max_message_bytes: 64,
        max_depth: 3,
        max_cmd_len: 8,
    };
    let _ = tpt_shell_ipc::parse_request(data, &tight);
}

/// Argument validator: the first byte picks a schema, the rest is read as JSON (or, when it is
/// not JSON, as a string). The validator must answer, not panic.
pub fn validator(data: &[u8]) {
    let schemas = schemas();
    let pick = data.first().map_or(0, |b| usize::from(*b)) % schemas.len();
    let rest = data.get(1..).unwrap_or_default();
    let value = serde_json::from_slice(rest)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(rest).into_owned()));
    let _ = validate(&schemas[pick], &value, "args");
}

/// Manifest parser: any text. Bytes that are not UTF-8 are refused by the target itself, since
/// the manifest is read as text.
pub fn manifest(data: &[u8]) {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = tpt_shell_manifest::parse(text);
    }
}

/// Audit chain verifier: the bytes are written to a scratch file and read back as a chain.
pub fn audit_chain(data: &[u8]) {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("tpt-fuzz-audit-{}-{n}.jsonl", std::process::id()));
    if std::fs::write(&path, data).is_ok() {
        let _ = tpt_shell_audit::verify_file(&path);
        let _ = std::fs::remove_file(&path);
    }
}

const LABEL: &[Field] = &[Field {
    name: "label",
    schema: Schema::Str {
        max_len: 16,
        chars: Some("ab-"),
    },
}];

/// The schemas the validator target chooses from. They cover every schema kind.
fn schemas() -> Vec<Schema> {
    vec![
        Schema::Struct(&[
            Field {
                name: "count",
                schema: Schema::Int { min: 0, max: 9 },
            },
            Field {
                name: "note",
                schema: Schema::Optional(&Schema::Bool),
            },
            Field {
                name: "slug",
                schema: Schema::Struct(LABEL),
            },
            Field {
                name: "tags",
                schema: Schema::List {
                    item: &Schema::Str {
                        max_len: 4,
                        chars: None,
                    },
                    max_len: 3,
                },
            },
            Field {
                name: "mode",
                schema: Schema::Enum(&["r", "w"]),
            },
            Field {
                name: "scores",
                schema: Schema::Map {
                    value: &Schema::Float { min: 0.0, max: 1.0 },
                    max_len: 2,
                },
            },
        ]),
        Schema::Enum(&["a", "b"]),
        Schema::List {
            item: &Schema::Int { min: -5, max: 5 },
            max_len: 4,
        },
        Schema::Str {
            max_len: 8,
            chars: None,
        },
        Schema::Bool,
    ]
}

/// Xorshift generator, so every run, and every failure, reproduces from its seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Zero is a fixed point of xorshift, so it is replaced.
        Rng(if seed == 0 {
            0x9e37_79b9_7f4a_7c15
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Random bytes, up to `max_len` of them.
    pub fn bytes(&mut self, max_len: usize) -> Vec<u8> {
        let len = (self.next_u64() as usize) % (max_len + 1);
        (0..len).map(|_| self.next_u64() as u8).collect()
    }

    /// `seed` with a few bytes flipped, inserted or dropped.
    pub fn mutate(&mut self, seed: &[u8]) -> Vec<u8> {
        let mut out = seed.to_vec();
        for _ in 0..=(self.next_u64() % 4) {
            if out.is_empty() {
                out.push(self.next_u64() as u8);
                continue;
            }
            let at = (self.next_u64() as usize) % out.len();
            match self.next_u64() % 3 {
                0 => out[at] = self.next_u64() as u8,
                1 => out.insert(at, self.next_u64() as u8),
                _ => {
                    out.remove(at);
                }
            }
        }
        out
    }
}

/// Inputs that have caught real parsers before: empty, lone delimiters, nesting, NULs, huge
/// numbers and lone surrogates.
pub fn fixed_inputs() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = [
        "",
        "{",
        "}",
        "[",
        "]]]]]",
        "{\"v\":1}",
        "{\"v\":1,\"id\":1,\"cmd\":\"x\",\"args\":{}}",
        "{\"v\":99999999999999999999999,\"id\":-1}",
        "\"\\ud800\"",
        "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]",
        "1e999999",
        "[app]\nname = \"demo\"\n",
        "[app]\nname = \"demo\"\n[[windows]]\nname = \"main\"\nwidth = 1e400\n",
        "[audit]\nredact = [\"a b\"]\n",
        "\u{0}\u{7f}\u{feff}",
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    out.push(vec![0xff, 0xfe, 0xfd]);
    out.push(vec![0; 300]);
    out
}

/// Realistic inputs to mutate: a valid envelope, a valid manifest and a valid audit record.
pub fn seeds() -> Vec<Vec<u8>> {
    vec![
        br#"{"v":1,"id":7,"cmd":"list_dir","args":{"path":"C:/Data","limit":3}}"#.to_vec(),
        b"[app]\nname = \"demo\"\nversion = \"1.0.0\"\n\n[[windows]]\nname = \"main\"\ntitle = \"Main\"\nwidth = 640\nheight = 480\n"
            .to_vec(),
        br#"{"seq":0,"actor":"page","command":"ping","outcome":"ok"}"#.to_vec(),
        concat!(
            "{\"app\":\"demo\",\"format\":\"tptreplay\",\"platform\":\"windows\",\"version\":2}\n",
            "{\"data\":{\"cmd\":\"ping\",\"id\":1},\"kind\":\"ipc_request\",\"seq\":0,\"t_ns\":0}\n",
            "{\"data\":{\"data\":\"pong\",\"id\":1,\"ok\":true},\"kind\":\"ipc_response\",\"seq\":1,\"t_ns\":5}\n",
        )
        .as_bytes()
        .to_vec(),
    ]
}

#[cfg(test)]
mod smoke {
    //! Deterministic runs of every target, over the fixed inputs and mutations of the seeds.

    use super::*;

    const ITERATIONS: usize = 3000;

    #[test]
    fn envelope_target_never_panics() {
        let mut rng = Rng::new(0x9e37_79b9_7f4a_7c15);
        for input in fixed_inputs() {
            envelope(&input);
        }
        for seed in seeds() {
            for _ in 0..ITERATIONS {
                envelope(&rng.mutate(&seed));
            }
        }
        for _ in 0..ITERATIONS {
            let n = rng.bytes(256);
            envelope(&n);
        }
    }

    #[test]
    fn validator_target_never_panics() {
        let mut rng = Rng::new(0x2545_f491_4f6c_dd1d);
        for input in fixed_inputs() {
            validator(&input);
        }
        for pick in 0..=u8::MAX {
            validator(&[pick]);
            validator(&[pick, b'{', b'}']);
        }
        for _ in 0..ITERATIONS {
            let n = rng.bytes(128);
            validator(&n);
        }
        let valid =
            br#"{"count":3,"slug":{"label":"ab-"},"tags":["x"],"mode":"r","scores":{"a":0.5}}"#;
        let mut input = vec![0u8];
        input.extend_from_slice(valid);
        validator(&input);
        for _ in 0..ITERATIONS {
            validator(&rng.mutate(&input));
        }
    }

    #[test]
    fn manifest_target_never_panics() {
        let mut rng = Rng::new(0xd1b5_4a32_d192_ed03);
        for input in fixed_inputs() {
            manifest(&input);
        }
        for seed in seeds() {
            for _ in 0..ITERATIONS {
                manifest(&rng.mutate(&seed));
            }
        }
    }

    #[test]
    fn audit_chain_target_never_panics() {
        let mut rng = Rng::new(0x0123_4567_89ab_cdef);
        for input in fixed_inputs() {
            audit_chain(&input);
        }
        // Fewer runs: each one writes and reads a file.
        for _ in 0..200 {
            let n = rng.bytes(512);
            audit_chain(&n);
        }
    }

    #[test]
    fn replay_target_never_panics() {
        let mut rng = Rng::new(0x6a09_e667_f3bc_c908);
        for input in fixed_inputs() {
            replay(&input);
        }
        for seed in seeds() {
            for _ in 0..ITERATIONS {
                replay(&rng.mutate(&seed));
            }
        }
        for _ in 0..ITERATIONS {
            let n = rng.bytes(256);
            replay(&n);
        }
    }

    #[test]
    fn the_validator_schemas_accept_a_good_value() {
        let value = serde_json::json!({
            "count": 3,
            "note": null,
            "slug": { "label": "ab-" },
            "tags": ["x"],
            "mode": "r",
            "scores": { "a": 0.5 }
        });
        assert!(validate(&schemas()[0], &value, "args").is_ok());
    }

    #[test]
    fn every_listed_target_is_runnable_and_unknown_names_are_refused() {
        for name in TARGETS {
            assert!(run_target(name, b"{}"), "{name}");
        }
        assert!(!run_target("nope", b"{}"));
    }
}
