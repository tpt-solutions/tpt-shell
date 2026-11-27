#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Offline docs bundle (spec §10.8, §15; todo "Runtime and air-gap": `tpt-shell docs --offline`).
# Builds the mdBook site in docs/, checks that the built pages load nothing from the network
# (tools/check-docs-offline.sh), and zips the result with tools/zip-dir.py. The zip can be opened
# from a file:// location on an air-gapped machine.
#
# usage: tools/docs-offline.sh [output.zip]     (default: target/docs-offline/tpt-shell-docs.zip)
#
# Needs: mdbook (not installed on the development machine yet; the CI docs job installs it),
# and python3 or python for the zip step (packaging/config.env probes for one).

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
. "$here/packaging/config.env"
zip_out="${1:-$here/target/docs-offline/tpt-shell-docs.zip}"

command -v mdbook >/dev/null 2>&1 \
  || { echo "mdbook not found; install it (cargo install mdbook, or the CI docs job)" >&2; exit 1; }
[ -f "$here/docs/book.toml" ] || { echo "no docs/book.toml; nothing to build" >&2; exit 1; }

# docs/book.toml runs its preprocessor as `python3 ...`. On Windows that name is often the Microsoft
# Store alias, which prints "Python was not found" and runs nothing, so the placeholders stay
# unrendered. When `python3` does not run, a shim named python3 that forwards to $PYTHON (probed in
# packaging/config.env) is put first on PATH for this build only. CI's Linux runners have python3,
# so the shim is not used there.
if ! python3 -c 'pass' >/dev/null 2>&1; then
  shim="$(mktemp -d)"
  if command -v cygpath >/dev/null 2>&1; then
    # Windows resolves a bare `python3` by appending .exe only, so the shim must be an .exe. A copy
    # of the interpreter finds its own DLLs through PATH, where the Python install already is.
    cp "$(command -v "$PYTHON")" "$shim/python3.exe" \
      || { echo "could not copy $PYTHON to $shim/python3.exe" >&2; exit 1; }
  else
    printf '#!/bin/sh\nexec "%s" "$@"\n' "$PYTHON" > "$shim/python3"
    chmod +x "$shim/python3"
  fi
  export PATH="$shim:$PATH"
  echo "python3 did not run; using $PYTHON through a shim for the docs preprocessor" >&2
fi

(cd "$here/docs" && mdbook build)
[ -z "${shim:-}" ] || rm -rf "$shim"
book="$here/docs/book"
"$here/tools/check-docs-offline.sh" "$book"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
mkdir -p "$staging/tpt-shell-docs"
cp -R "$book/." "$staging/tpt-shell-docs/"
mkdir -p "$(dirname "$zip_out")"
"$PYTHON" -I "$here/tools/zip-dir.py" "$staging/tpt-shell-docs" "$zip_out"
echo "offline docs bundle: $zip_out"
