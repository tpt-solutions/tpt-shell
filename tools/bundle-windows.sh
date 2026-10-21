#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Windows bundle (spec §10.8, §16 M10): a portable folder, a zip of it, and optionally an installer
# built by an external tool (todo O2; Inno Setup by default, see packaging/config.env). Nothing is
# signed here: run tools/sign.sh windows on the outputs when the signing credentials exist.
#
# The portable folder layout:
#   <App>-<version>-windows-x64/
#     <binary>.exe            the release binary (tpt-shell build)
#     WebView2Loader.dll      the pinned loader (THIRD_PARTY_NOTICES, decision O1), if given
#     app/                    the front-end files
#     runtime/                pinned WebView2 Fixed Version runtime, if given (tpt-shell runtime pin)
#     compliance/             sbom.*.json, attestation.json, THIRD_PARTY_NOTICES, licenses.html
#     LICENSE-APACHE, LICENSE-MIT, tpt-shell.toml (if given)
#
# usage: tools/bundle-windows.sh --binary <exe> --app-dir <dir> --version <v> --out <dir>
#          [--loader <WebView2Loader.dll>] [--runtime <fixed-runtime-dir>]
#          [--compliance <dir>] [--manifest <tpt-shell.toml>] [--installer]
#
# --installer runs the tool named by TPT_WIN_INSTALLER_TOOL (default iscc, Inno Setup 6) on
# packaging/windows/installer.iss. Without the flag only the folder and zip are written.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
. "$here/packaging/config.env"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

binary=""; app_dir=""; version=""; out=""; loader=""; runtime=""
compliance=""; manifest=""; installer=0
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --app-dir) app_dir="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --loader) loader="$2"; shift 2 ;;
    --runtime) runtime="$2"; shift 2 ;;
    --compliance) compliance="$2"; shift 2 ;;
    --manifest) manifest="$2"; shift 2 ;;
    --installer) installer=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$binary" ] && [ -n "$app_dir" ] && [ -n "$version" ] && [ -n "$out" ] || usage

need_file() { [ -f "$1" ] || { echo "missing file: $1" >&2; exit 1; }; }
need_dir() { [ -d "$1" ] || { echo "missing directory: $1" >&2; exit 1; }; }
need_file "$binary"
need_dir "$app_dir"
[ -z "$loader" ] || need_file "$loader"
[ -z "$runtime" ] || need_dir "$runtime"
[ -z "$compliance" ] || need_dir "$compliance"
[ -z "$manifest" ] || need_file "$manifest"
if [ -n "$runtime" ] && [ ! -f "$runtime/msedgewebview2.exe" ]; then
  echo "runtime folder has no msedgewebview2.exe: $runtime" >&2
  exit 1
fi

name="$APP_NAME-$version-windows-x64"
stage="$out/$name"
rm -rf "$stage"
mkdir -p "$stage"

cp "$binary" "$stage/$APP_BINARY.exe"
[ -z "$loader" ] || cp "$loader" "$stage/WebView2Loader.dll"
cp -R "$app_dir" "$stage/app"
[ -z "$runtime" ] || cp -R "$runtime" "$stage/runtime"
[ -z "$compliance" ] || cp -R "$compliance" "$stage/compliance"
[ -z "$manifest" ] || cp "$manifest" "$stage/tpt-shell.toml"
for licence in LICENSE-APACHE LICENSE-MIT; do
  [ -f "$here/$licence" ] && cp "$here/$licence" "$stage/$licence" || true
done

zip="$out/$name.zip"
"$PYTHON" -I "$here/tools/zip-dir.py" "$stage" "$zip"

if [ "$installer" -eq 1 ]; then
  tool="$TPT_WIN_INSTALLER_TOOL"
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "installer tool not found: $tool (Inno Setup 6 provides ISCC.exe; see packaging/config.env)" >&2
    exit 1
  fi
  case "$tool" in
    iscc|ISCC|ISCC.exe|*/ISCC.exe)
      "$tool" \
        "/DAppName=$APP_NAME" \
        "/DAppBinary=$APP_BINARY" \
        "/DAppVersion=$version" \
        "/DAppPublisher=$PUBLISHER" \
        "/DAppId=$APP_INSTALLER_GUID" \
        "/DSourceDir=$stage" \
        "/DOutputDir=$out" \
        "$here/packaging/windows/installer.iss"
      ;;
    *)
      echo "no command template for installer tool '$tool'; add one to tools/bundle-windows.sh" >&2
      exit 1
      ;;
  esac
  echo "installer written to $out"
fi

(cd "$out" && sha256sum "$name.zip" >> SHA256SUMS.windows)
echo "portable folder: $stage"
echo "zip: $zip"
echo "checksums: $out/SHA256SUMS.windows"
echo "not signed. Run tools/sign.sh windows on the binary and installer when credentials exist."
