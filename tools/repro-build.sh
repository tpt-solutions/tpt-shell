#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# One reproducible release build (spec §10.8; guidance in packaging/repro/REPRODUCIBLE.md). It sets
# the flags that make two builds of the same commit on the same toolchain produce the same bytes:
#   SOURCE_DATE_EPOCH   the commit time, unless the caller set one
#   CARGO_INCREMENTAL=0 no incremental state in the output
#   --remap-path-prefix the checkout, CARGO_HOME and the Rust sysroot are replaced by fixed names,
#                       so absolute paths do not end up in the binary
#   -C link-arg=/Brepro (Windows only) a fixed PE timestamp instead of the link time
#   --locked            the committed Cargo.lock is used as it is
#
# usage: tools/repro-build.sh <target-dir> [cargo build arguments...]
#   default arguments: -p tpt-shell-cli --bin tpt-shell
#   Prints the path of the binary and its SHA-256.
#
# Needs: cargo, rustc, git. On Windows, cygpath (Git Bash) is used to give rustc native paths.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
[ $# -ge 1 ] || { sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
target="$1"; shift
if [ $# -eq 0 ]; then set -- -p tpt-shell-cli --bin tpt-shell; fi

native() {
  if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi
}

if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
  SOURCE_DATE_EPOCH="$(git -C "$here" log -1 --format=%ct HEAD)"
fi
export SOURCE_DATE_EPOCH
export CARGO_INCREMENTAL=0

cargo_home="${CARGO_HOME:-$HOME/.cargo}"
sysroot="$(rustc --print sysroot)"
flags="${RUSTFLAGS:-}"
flags="$flags --remap-path-prefix=$(native "$here")=/build/src"
flags="$flags --remap-path-prefix=$(native "$cargo_home")=/build/cargo-home"
flags="$flags --remap-path-prefix=$(native "$sysroot")=/build/rust"
if rustc -vV | grep -q '^host: .*msvc'; then
  flags="$flags -C link-arg=/Brepro"
fi
export RUSTFLAGS="$flags"

cd "$here"
export CARGO_TARGET_DIR="$target"
cargo build --release --locked "$@"

bin_dir="$target/release"
bin=""
for candidate in "$bin_dir"/tpt-shell.exe "$bin_dir"/tpt-shell; do
  if [ -f "$candidate" ]; then bin="$candidate"; break; fi
done
[ -n "$bin" ] || { echo "no release binary found in $bin_dir (check the cargo arguments)" >&2; exit 1; }
echo "binary: $bin"
sha256sum "$bin"
