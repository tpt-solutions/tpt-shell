#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# WebView2 Fixed Version runtime: fetch, unpack, record, and list (spec §10.8, todo "Runtime and
# air-gap": `tpt-shell runtime pin | list`). Runs on the DEVELOPER machine only. The shipped app
# never contacts Microsoft; it loads the folder this script produces.
#
# Subcommands:
#   pin  --cab <file.cab> | --url <https-url> --out <runtime-dir> [--sha256 <hex>]
#        Gets the Fixed Version CAB (from a local file, or downloaded with curl), checks its
#        SHA-256 when --sha256 is given, unpacks it with expand.exe into <runtime-dir>, checks that
#        msedgewebview2.exe is present, and writes <runtime-dir>/webview2-fixed.toml with the
#        fields in packaging/runtime/webview2-fixed.example.toml.
#   list <runtime-dir>...
#        Reads each webview2-fixed.toml and checks that its folder still holds msedgewebview2.exe
#        and that the recorded version matches the folder name.
#
# The CAB comes from Microsoft's WebView2 download page (Fixed Version, x64). The URL is not
# hard-coded here: pass it with --url or take the file you downloaded with --cab. Use --sha256 so
# the pin is checked against the value you recorded when you chose the version.
#
# usage: tools/runtime-pin.sh pin (--cab <file> | --url <url>) --out <dir> [--sha256 <hex>]
#        tools/runtime-pin.sh list <dir>...
# Needs: sha256sum, sed, and on Windows expand.exe (Git Bash has it through System32).

set -euo pipefail

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

sha256_of() { sha256sum "$1" | cut -d' ' -f1; }

cmd_pin() {
  local cab="" url="" out="" want=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --cab) cab="$2"; shift 2 ;;
      --url) url="$2"; shift 2 ;;
      --out) out="$2"; shift 2 ;;
      --sha256) want="$2"; shift 2 ;;
      *) echo "unknown argument: $1" >&2; usage ;;
    esac
  done
  [ -n "$out" ] || usage
  if [ -n "$cab" ] && [ -n "$url" ]; then echo "give --cab or --url, not both" >&2; exit 2; fi
  [ -n "$cab$url" ] || usage

  local work
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' RETURN
  if [ -n "$url" ]; then
    case "$url" in https://*) ;; *) echo "--url must be https" >&2; exit 2 ;; esac
    cab="$work/runtime.cab"
    curl -fsSL --proto '=https' --tlsv1.2 -o "$cab" "$url"
  fi
  [ -f "$cab" ] || { echo "missing CAB: $cab" >&2; exit 1; }

  local digest
  digest="$(sha256_of "$cab")"
  echo "CAB sha256: $digest"
  if [ -n "$want" ] && [ "$want" != "$digest" ]; then
    echo "sha256 mismatch: expected $want, got $digest" >&2
    exit 1
  fi

  rm -rf "$out"
  mkdir -p "$out"
  # Use the System32 copy explicitly: on Git Bash a coreutils `expand` can shadow it and rejects -F.
  local expander="${SYSTEMROOT:-/c/Windows}/System32/expand.exe"
  [ -x "$expander" ] || { echo "System32 expand.exe not found (Windows only)" >&2; exit 1; }
  "$expander" -F:*"$(cygpath -w "$cab" 2>/dev/null || echo "$cab")" \
    "$(cygpath -w "$out" 2>/dev/null || echo "$out")" >/dev/null

  # The Fixed Version CAB unpacks to one folder named after the version, for example
  # Microsoft.WebView2.FixedVersionRuntime.128.0.2739.42.x64, which holds msedgewebview2.exe.
  local exe version_dir version
  exe="$(find "$out" -name msedgewebview2.exe -type f | head -n 1)"
  [ -n "$exe" ] || { echo "msedgewebview2.exe not found after unpacking" >&2; exit 1; }
  version_dir="$(dirname "$exe")"
  local folder version
  folder="$(basename "$version_dir")"
  version="$(printf '%s' "$folder" | sed -E 's/.*\.([0-9]+\.[0-9]+\.[0-9]+\.[0-9]+)\..*$/\1/')"
  case "$version" in
    [0-9]*.[0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "cannot read the version from folder name $folder" >&2; exit 1 ;;
  esac

  cat > "$out/webview2-fixed.toml" <<EOF
# Written by tools/runtime-pin.sh. Read by tools/runtime-pin.sh list and, once the crate supports
# it, by the bundler. Do not edit the sha256 by hand.
[runtime.webview2]
mode = "fixed"
version = "$version"
folder = "$folder"
executable = "msedgewebview2.exe"
cab_sha256 = "$digest"
source = "${url:-local file}"
fetched_utc = "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EOF
  echo "pinned WebView2 Fixed Version $version in $out"
  echo "record: $out/webview2-fixed.toml"
}

cmd_list() {
  [ $# -ge 1 ] || usage
  local dir record folder version exe bad=0
  for dir in "$@"; do
    record="$dir/webview2-fixed.toml"
    if [ ! -f "$record" ]; then
      echo "no pin in $dir"
      bad=1
      continue
    fi
    version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$record")"
    folder="$(sed -n 's/^folder = "\(.*\)"$/\1/p' "$record")"
    exe="$dir/$folder/msedgewebview2.exe"
    if [ -f "$exe" ] && [ -n "$version" ] && case "$folder" in *"$version"*) true ;; *) false ;; esac; then
      echo "ok   $dir  WebView2 $version"
    else
      echo "BAD  $dir  WebView2 $version (folder or executable missing, or version does not match)"
      bad=1
    fi
  done
  return "$bad"
}

[ $# -ge 1 ] || usage
sub="$1"; shift
case "$sub" in
  pin) cmd_pin "$@" ;;
  list) cmd_list "$@" ;;
  *) usage ;;
esac
