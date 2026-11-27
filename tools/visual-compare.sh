#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Compares screenshots from the CI jobs with a committed baseline (spec §10.6, `tpt-shell visual
# compare`). It runs the comparison only when both exist, and otherwise says why it skipped and
# exits 0, so the gap shows in the log rather than failing the build.
#
# A comparison that fails exits 1 and fails the job. The HTML report is written to $REPORT
# (default visual-report.html) whether or not the comparison passes.
#
# usage: tools/visual-compare.sh <baseline-dir> <screenshot-dir>...
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: tools/visual-compare.sh <baseline-dir> <screenshot-dir>..." >&2
  exit 2
fi
baseline="$1"
shift

has_png() {
  [ -d "$1" ] && find "$1" -name '*.png' -print -quit | grep -q .
}

if ! has_png "$baseline"; then
  echo "visual-compare: skipped, no baseline screenshots in $baseline"
  exit 0
fi

dirs=()
for dir in "$@"; do
  if has_png "$dir"; then
    dirs+=("$dir")
  fi
done
if [ "${#dirs[@]}" -eq 0 ]; then
  echo "visual-compare: skipped, no screenshots to compare against $baseline"
  exit 0
fi

report="${REPORT:-visual-report.html}"
cargo run --locked -q -p tpt-shell-cli -- visual compare "$baseline" "${dirs[@]}" --out "$report"
