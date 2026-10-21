#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Reproducibility test (spec §16 M10 acceptance; todo "Reproducibility test"). Builds the tpt-shell
# CLI twice from the same commit with tools/repro-build.sh, each into its own target directory,
# runs `tpt-shell compliance` on both builds, and compares the binary and every compliance output
# by SHA-256. Exit 0 when all match. Exit 1 lists what differs, so the difference can be explained
# in docs/ (packaging/repro/REPRODUCIBLE.md) instead of being hidden.
#
# Meant for CI, on the same runner image as the release job. Each build is a full build, so it is
# slow; this script never runs `cargo clean` and leaves both target directories in place.
#
# usage: tools/repro-check.sh [work-directory]     (default: target/repro)

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
work="${1:-$here/target/repro}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"

# One epoch for both builds, so timestamps in the compliance outputs match.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
  SOURCE_DATE_EPOCH="$(git -C "$here" log -1 --format=%ct HEAD)"
fi
export SOURCE_DATE_EPOCH

build_side() {
  local side="$1"
  local target="$work/$side/target"
  mkdir -p "$work/$side"
  "$here/tools/repro-build.sh" "$target" >"$work/$side/build.log" 2>&1 \
    || { cat "$work/$side/build.log" >&2; echo "build $side failed" >&2; exit 1; }
  local bin="$target/release/tpt-shell"
  [ -f "$bin.exe" ] && bin="$bin.exe"
  cp "$bin" "$work/$side/tpt-shell-binary"
  # A failing licence gate exits 1 but still writes the outputs, so only a missing output is fatal.
  (cd "$here" && "$bin" compliance "$work/$side/compliance") >"$work/$side/compliance.log" 2>&1 || true
  [ -f "$work/$side/compliance/attestation.json" ] \
    || { cat "$work/$side/compliance.log" >&2; echo "compliance failed for $side" >&2; exit 1; }
}

build_side a
build_side b

status=0
diff_list="$work/differences.txt"
: > "$diff_list"

if cmp -s "$work/a/tpt-shell-binary" "$work/b/tpt-shell-binary"; then
  echo "binary: identical"
else
  echo "binary: DIFFERENT" | tee -a "$diff_list"
  status=1
fi

if diff -r "$work/a/compliance" "$work/b/compliance" >"$work/compliance.diff" 2>&1; then
  echo "compliance outputs: identical"
else
  echo "compliance outputs: DIFFERENT (see $work/compliance.diff)" | tee -a "$diff_list"
  status=1
fi

# attestation.json holds the digests of the SBOMs, so differing SBOMs show up there too.
sha256sum "$work/a/tpt-shell-binary" "$work/b/tpt-shell-binary"
if [ "$status" -eq 0 ]; then
  echo "reproducible: both builds match"
else
  echo "not reproducible: explain each difference in docs before release (see $diff_list)" >&2
fi
exit "$status"
