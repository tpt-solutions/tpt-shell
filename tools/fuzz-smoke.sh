#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Runs every fuzz target for a fixed number of inputs with a fixed seed (spec §14, "Property and
# fuzz tests"). The run is deterministic, so a failure on one commit reproduces on the next. It is
# a smoke test, not a search: the long runs are the M12 fuzzing item and the nightly job.
#
# The targets live in fuzz/ (cargo-fuzz). Each target is one `fuzz_target!` in fuzz/fuzz_targets.
# Until fuzz/ exists the script says so and exits 0; the CI job then shows a clear skip.
#
# usage: tools/fuzz-smoke.sh [runs-per-target]   (default 2000)

set -euo pipefail

runs="${1:-2000}"
root="$(cd "$(dirname "$0")/.." && pwd)"
fuzz_dir="$root/fuzz"

if [ ! -f "$fuzz_dir/Cargo.toml" ]; then
  echo "no fuzz targets yet: fuzz/ does not exist (the contract agent adds it with the validator)"
  echo "skipping the fuzz smoke run"
  exit 0
fi

if ! cargo fuzz --version >/dev/null 2>&1; then
  echo "cargo-fuzz is not installed; install it with cargo install cargo-fuzz or taiki-e/install-action" >&2
  exit 1
fi

cd "$fuzz_dir"
targets=$(cargo fuzz list)
if [ -z "$targets" ]; then
  echo "fuzz/ has no targets yet; nothing to run"
  exit 0
fi

status=0
for target in $targets; do
  echo "== fuzz smoke: $target ($runs runs, seed 1)"
  # -seed fixes the input sequence; -runs bounds the run; -max_total_time is a safety net.
  if ! cargo +nightly fuzz run "$target" -- -runs="$runs" -seed=1 -max_total_time=300; then
    echo "fuzz target $target failed; its crash input is under fuzz/artifacts/$target" >&2
    status=1
  fi
done
exit "$status"
