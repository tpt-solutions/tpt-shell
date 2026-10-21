#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# macOS bundle (spec §10.8, §16 M10): an .app bundle, a .dmg (optional) and a tar.gz of the .app.
# Written for macOS and NOT run here: this host is Windows. Checked with `bash -n` only; the first
# real run is on macOS hardware or the macOS CI runner (todo O6). Nothing is signed or notarised:
# run tools/sign.sh macos when the credentials exist, and sign before making the .dmg.
#
# The .app layout:
#   <App>.app/Contents/Info.plist       from packaging/macos/Info.plist.in
#   <App>.app/Contents/MacOS/<binary>   the release binary (use lipo for a universal build)
#   <App>.app/Contents/Resources/app/   the front-end files
#   <App>.app/Contents/Resources/AppIcon.icns  if --icns is given
#   <App>.app/Contents/Resources/compliance/   SBOMs, attestation, notices if --compliance is given
#
# usage: tools/bundle-macos.sh --binary <bin> --app-dir <dir> --version <v> --arch <arm64|x86_64|universal>
#          --out <dir> [--icns <file>] [--compliance <dir>] [--dmg]
#
# Needs: sed, tar, gzip (BSD tar, COPYFILE_DISABLE stops AppleDouble files) and, for --dmg, hdiutil.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
. "$here/packaging/config.env"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

binary=""; app_dir=""; version=""; arch=""; out=""; icns=""; compliance=""; dmg=0
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --app-dir) app_dir="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    --arch) arch="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --icns) icns="$2"; shift 2 ;;
    --compliance) compliance="$2"; shift 2 ;;
    --dmg) dmg=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$binary" ] && [ -n "$app_dir" ] && [ -n "$version" ] && [ -n "$arch" ] && [ -n "$out" ] || usage
case "$arch" in arm64|x86_64|universal) ;; *) echo "bad --arch: $arch" >&2; exit 2 ;; esac

[ -f "$binary" ] || { echo "missing binary: $binary" >&2; exit 1; }
[ -d "$app_dir" ] || { echo "missing app directory: $app_dir" >&2; exit 1; }

app="$out/$APP_NAME.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/$APP_BINARY"
chmod 755 "$app/Contents/MacOS/$APP_BINARY"
cp -R "$app_dir" "$app/Contents/Resources/app"
[ -z "$icns" ] || cp "$icns" "$app/Contents/Resources/AppIcon.icns"
[ -z "$compliance" ] || cp -R "$compliance" "$app/Contents/Resources/compliance"

sed \
  -e "s|@APP_BINARY@|$APP_BINARY|g" \
  -e "s|@APP_BUNDLE_ID@|$APP_BUNDLE_ID|g" \
  -e "s|@APP_NAME@|$APP_NAME|g" \
  -e "s|@VERSION@|$version|g" \
  -e "s|@MIN_MACOS@|$MIN_MACOS|g" \
  -e "s|@PUBLISHER@|$PUBLISHER|g" \
  "$here/packaging/macos/Info.plist.in" > "$app/Contents/Info.plist"

stem="$APP_NAME-$version-macos-$arch"

# tar.gz of the .app. COPYFILE_DISABLE=1 keeps AppleDouble files out (bsdtar), and
# gzip -n drops the timestamp so the archive bytes do not depend on when it was made.
tgz="$out/$stem.tar.gz"
COPYFILE_DISABLE=1 tar -C "$out" -cf - "$APP_NAME.app" | gzip -n > "$tgz"
echo "tar.gz: $tgz"

if [ "$dmg" -eq 1 ]; then
  command -v hdiutil >/dev/null 2>&1 || { echo "hdiutil not found; --dmg needs macOS" >&2; exit 1; }
  image="$out/$stem.dmg"
  rm -f "$image"
  hdiutil create -volname "$APP_NAME" -srcfolder "$app" -ov -format UDZO "$image"
  echo "dmg: $image"
fi

(cd "$out" && shasum -a 256 "$stem".* > SHA256SUMS.macos 2>/dev/null || sha256sum "$stem".* > SHA256SUMS.macos)
echo ".app: $app"
echo "not signed or notarised. Run tools/sign.sh macos before distributing."
