#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Signing hooks (spec §10.8, §1 "sign": Windows code signing, macOS notarisation, minisign for
# archives). Each hook runs ONLY when its credentials are present in the environment. With none
# set, it prints what it skipped and exits 0, so CI can call it on every build and it signs only
# where a release job has supplied secrets. Nothing here has been run against real credentials.
#
# usage: tools/sign.sh <windows|macos|minisign|check> [--require] <file-or-app>...
#   check     lists which signing tools and credentials are present; signs nothing
#   --require make a missing credential an error (exit 1) instead of a skip
#
# Environment (all optional; a hook is skipped unless its group is complete):
#   windows (signtool.exe, Windows SDK):
#     TPT_WIN_SIGN_PFX           path to a .pfx certificate file
#     TPT_WIN_SIGN_PFX_PASSWORD  its password (passed to signtool /p; visible to the process list,
#                                so prefer a hardware token or cloud signing, see below)
#     TPT_WIN_SIGN_DLIB, TPT_WIN_SIGN_DMDF   cloud signing (Azure Trusted Signing): the dlib and
#                                metadata file. Used instead of TPT_WIN_SIGN_PFX when both are set.
#     TPT_WIN_TIMESTAMP_URL      RFC 3161 timestamp server (required whenever signing)
#   macos (codesign, notarytool, stapler; Xcode command line tools, run on macOS):
#     TPT_MAC_SIGN_IDENTITY      "Developer ID Application: ..." identity in the keychain
#     TPT_MAC_NOTARY_PROFILE     a keychain profile created with `xcrun notarytool store-credentials`
#                                (no password in the environment)
#     TPT_MAC_ENTITLEMENTS       optional entitlements plist
#   minisign (https://jedisct1.github.io/minisign/):
#     TPT_MINISIGN_KEY           path to the secret key file
#     TPT_MINISIGN_PUB           path to the public key; when set, each signature is verified
#     minisign prompts for the key password on a terminal. A non-interactive release job needs a
#     key without a password, stored as a CI secret (see the trade-off in packaging/README.md).
#
# Signatures are written next to each file: <file>.minisig for minisign. Windows and macOS
# sign in place.

set -euo pipefail

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

[ $# -ge 1 ] || usage
mode="$1"; shift
require=0
if [ "${1:-}" = "--require" ]; then require=1; shift; fi

skip() {
  if [ "$require" -eq 1 ]; then
    echo "required signing credentials missing: $1" >&2
    exit 1
  fi
  echo "skipped ($mode): $1" >&2
}

have() { command -v "$1" >/dev/null 2>&1; }

check_tools() {
  for t in signtool codesign xcrun minisign; do
    if have "$t"; then echo "tool present: $t"; else echo "tool missing: $t"; fi
  done
  for v in TPT_WIN_SIGN_PFX TPT_WIN_SIGN_DLIB TPT_MAC_SIGN_IDENTITY TPT_MAC_NOTARY_PROFILE TPT_MINISIGN_KEY; do
    if [ -n "${!v:-}" ]; then echo "credential set: $v"; else echo "credential not set: $v"; fi
  done
}

sign_windows() {
  if [ -n "${TPT_WIN_SIGN_DLIB:-}" ] && [ -n "${TPT_WIN_SIGN_DMDF:-}" ]; then
    :
  elif [ -n "${TPT_WIN_SIGN_PFX:-}" ]; then
    :
  else
    skip "set TPT_WIN_SIGN_PFX or TPT_WIN_SIGN_DLIB and TPT_WIN_SIGN_DMDF"
    return 0
  fi
  [ -n "${TPT_WIN_TIMESTAMP_URL:-}" ] || { skip "TPT_WIN_TIMESTAMP_URL is not set"; return 0; }
  have signtool || { echo "signtool not found (Windows SDK)" >&2; exit 1; }
  for f in "$@"; do
    if [ -n "${TPT_WIN_SIGN_DLIB:-}" ] && [ -n "${TPT_WIN_SIGN_DMDF:-}" ]; then
      signtool sign /fd SHA256 /tr "$TPT_WIN_TIMESTAMP_URL" /td SHA256 \
        /dlib "$TPT_WIN_SIGN_DLIB" /dmdf "$TPT_WIN_SIGN_DMDF" "$f"
    else
      pw=()
      [ -z "${TPT_WIN_SIGN_PFX_PASSWORD:-}" ] || pw=(/p "$TPT_WIN_SIGN_PFX_PASSWORD")
      signtool sign /fd SHA256 /f "$TPT_WIN_SIGN_PFX" ${pw[@]+"${pw[@]}"} \
        /tr "$TPT_WIN_TIMESTAMP_URL" /td SHA256 "$f"
    fi
    signtool verify /pa "$f"
    echo "signed: $f"
  done
}

sign_macos() {
  if [ -z "${TPT_MAC_SIGN_IDENTITY:-}" ]; then
    skip "TPT_MAC_SIGN_IDENTITY is not set"
    return 0
  fi
  have codesign || { echo "codesign not found (macOS only)" >&2; exit 1; }
  for app in "$@"; do
    ent=()
    [ -z "${TPT_MAC_ENTITLEMENTS:-}" ] || ent=(--entitlements "$TPT_MAC_ENTITLEMENTS")
    # Sign nested executables first, then the bundle, with the hardened runtime and a timestamp.
    # Nested code is not signed with --deep, which hides signing errors.
    if [ -d "$app" ]; then
      while IFS= read -r nested; do
        codesign --force --options runtime --timestamp --sign "$TPT_MAC_SIGN_IDENTITY" ${ent[@]+"${ent[@]}"} "$nested"
      done < <(find "$app/Contents" -type f \( -perm -u+x -o -name '*.dylib' \) | sort)
    fi
    codesign --force --options runtime --timestamp --sign "$TPT_MAC_SIGN_IDENTITY" ${ent[@]+"${ent[@]}"} "$app"
    codesign --verify --strict --verbose=2 "$app"
    echo "signed: $app"
    if [ -n "${TPT_MAC_NOTARY_PROFILE:-}" ]; then
      zip_path="$(mktemp -d)/notarise.zip"
      ditto -c -k --keepParent "$app" "$zip_path"
      xcrun notarytool submit "$zip_path" --keychain-profile "$TPT_MAC_NOTARY_PROFILE" --wait
      xcrun stapler staple "$app"
      xcrun stapler validate "$app"
      echo "notarised and stapled: $app"
    else
      echo "notarisation skipped: TPT_MAC_NOTARY_PROFILE is not set" >&2
    fi
  done
}

sign_minisign() {
  if [ -z "${TPT_MINISIGN_KEY:-}" ]; then
    skip "TPT_MINISIGN_KEY is not set"
    return 0
  fi
  have minisign || { echo "minisign not found" >&2; exit 1; }
  [ -f "$TPT_MINISIGN_KEY" ] || { echo "no key file at $TPT_MINISIGN_KEY" >&2; exit 1; }
  for f in "$@"; do
    minisign -S -s "$TPT_MINISIGN_KEY" -m "$f" -x "$f.minisig" -t "$(basename "$f") (tpt-shell release)"
    if [ -n "${TPT_MINISIGN_PUB:-}" ]; then
      minisign -V -p "$TPT_MINISIGN_PUB" -m "$f" -x "$f.minisig"
    fi
    echo "signed: $f -> $f.minisig"
  done
}

case "$mode" in
  check) check_tools ;;
  windows|macos|minisign)
    [ $# -ge 1 ] || { echo "no files given" >&2; exit 2; }
    "sign_$mode" "$@" ;;
  *) usage ;;
esac
