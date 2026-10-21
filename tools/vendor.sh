#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Vendors the dependencies for an air-gapped build (spec §10.8, todo "Runtime and air-gap":
# `tpt-shell vendor`). Runs `cargo vendor --locked` into a directory, then writes two files beside
# it: a record of what was vendored (sha256 of the lock file, of every checksum file, the cargo
# version and the crate count), and the source-replacement snippet to paste into .cargo/config.toml.
# The script does not edit any .cargo/config.toml itself.
#
# usage: tools/vendor.sh [vendor-directory]   (relative paths are from the repo root; default target/vendor)
#   Writes <vendor-directory> and <vendor-directory>.record.json and <vendor-directory>.config.toml.
#
# Needs: cargo (with the workspace Cargo.lock present), sha256sum, find, sort.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:-$here/target/vendor}"
lock="$here/Cargo.lock"
[ -f "$lock" ] || { echo "no Cargo.lock in $here; vendoring needs the committed lock file" >&2; exit 1; }

cd "$here"
rm -rf "$dest"
mkdir -p "$(dirname "$dest")"
# --locked: fail if Cargo.lock would change. The versions vendored are the ones in the lock file.
cargo vendor --locked "$dest" >/dev/null

# Source-replacement snippet. cargo vendor prints it; keep it in its own file for pasting.
cat > "$dest.config.toml" <<EOF
# Paste into .cargo/config.toml (or merge into it) to build from the vendored sources only.
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "$(printf '%s' "$dest" | sed 's#\\#/#g')"
EOF

# A digest over the checksum files gives one value that changes if any vendored crate changes.
tree_digest="$(cd "$dest" && find . -name .cargo-checksum.json -type f | sort \
  | xargs sha256sum | sha256sum | cut -d' ' -f1)"
crates="$(find "$dest" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
lock_digest="$(sha256sum "$lock" | cut -d' ' -f1)"
cargo_version="$(cargo --version)"
commit="$(git -C "$here" rev-parse HEAD 2>/dev/null || echo unknown)"

cat > "$dest.record.json" <<EOF
{
  "tool": "tools/vendor.sh",
  "cargo": "$cargo_version",
  "git_commit": "$commit",
  "cargo_lock_sha256": "$lock_digest",
  "crate_count": $crates,
  "checksum_tree_sha256": "$tree_digest",
  "vendor_directory": "$(printf '%s' "$dest" | sed 's#\\#/#g')",
  "flags": ["--locked"]
}
EOF

echo "vendored $crates crates into $dest"
echo "record: $dest.record.json"
echo "config snippet: $dest.config.toml"
