#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Runs every fuzz target for a fixed time each (spec §14, "Property and fuzz tests"). It is a smoke
# test, not a search: the long runs are the M12 fuzzing item and the nightly job.
#
# The targets live in fuzz/ and run on stable Rust through the mutation driver in fuzz/src/main.rs.
# cargo-fuzz is not used: libfuzzer-sys is licensed under a term the licence allow list does not
# permit (see fuzz/Cargo.toml). The driver is seeded from the clock, so a run is not reproducible.
# A crash is written to fuzz/crashes/<target>-<run>.bin and the target fails.
#
# Until fuzz/ exists the script says so and exits 0; the CI job then shows a clear skip.
#
# usage: tools/fuzz-smoke.sh [seconds-per-target]   (default 30)

set -euo pipefail

seconds="${1:-30}"
root="$(cd "$(dirname "$0")/.." && pwd)"
fuzz_dir="$root/fuzz"

if [ ! -f "$fuzz_dir/Cargo.toml" ]; then
  echo "no fuzz targets yet: fuzz/ does not exist (the contract agent adds it with the validator)"
  echo "skipping the fuzz smoke run"
  exit 0
fi

# The target names are the TARGETS list in the fuzz library, so the two cannot drift apart.
targets=$(sed -n 's/^pub const TARGETS.*= \[\(.*\)\];/\1/p' "$fuzz_dir/src/lib.rs" | tr -d '"' | tr ',' '\n' | tr -d ' ')
if [ -z "$targets" ]; then
  echo "fuzz/ has no targets yet; nothing to run"
  exit 0
fi

cargo build --quiet --release --manifest-path "$fuzz_dir/Cargo.toml"

status=0
for target in $targets; do
  echo "== fuzz smoke: $target ($seconds s)"
  if ! cargo run --quiet --release --manifest-path "$fuzz_dir/Cargo.toml" -- "$target" "$seconds"; then
    echo "fuzz target $target failed; its crash input is under fuzz/crashes/" >&2
    status=1
  fi
done
exit "$status"
