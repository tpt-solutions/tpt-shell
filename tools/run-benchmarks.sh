#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Runs the measurements that exist today and writes them to target/bench/results.json (spec §13).
# The CI benchmarks job runs this on Windows and uploads the file. Methodology: BENCHMARKS.md.
#
# Measured now:
#   - hello-world binary size (release build of examples/hello), budget 4 MB (spec §13)
#
# Not measured yet, and written as "pending" in the results file, so nothing is claimed:
#   cold start, IPC round trip, blob throughput, idle memory, supervisor memory. The IPC and
#   blob numbers come from the conformance suite (conformance/out), which the job uploads with
#   this file.
#
# usage: tools/run-benchmarks.sh

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/target/bench"
mkdir -p "$out"

cargo build --release --locked --manifest-path "$root/Cargo.toml" -p tpt-shell --example hello

exe="$root/target/release/examples/hello.exe"
[ -f "$exe" ] || exe="$root/target/release/examples/hello"
if [ ! -f "$exe" ]; then
  echo "the hello example was not built" >&2
  exit 1
fi

size=$(wc -c < "$exe" | tr -d ' ')
budget=4194304
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) os=windows ;;
  Darwin) os=macos ;;
  *) os=linux ;;
esac
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

cat > "$out/results.json" <<JSON
{
  "os": "$os",
  "measured_at": "$stamp",
  "binary_size": {
    "file": "$(basename "$exe")",
    "bytes": $size,
    "budget_bytes": $budget,
    "within_budget": $([ "$size" -le "$budget" ] && echo true || echo false)
  },
  "pending": [
    "cold start to first paint (budget 500 ms)",
    "IPC round trip p95 (budget 5 ms): see conformance/out",
    "blob throughput (budget 50 MB/s): see conformance/out",
    "idle shell memory (budget 25 MB)",
    "supervisor idle memory (budget 5 MB)"
  ]
}
JSON

echo "hello binary: $size bytes (budget $budget)"
echo "wrote $out/results.json"
