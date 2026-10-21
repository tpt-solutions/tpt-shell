#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Linux bundle (spec §10.8, §16 M10): tar.gz always; .deb and .rpm with nfpm; AppImage with
# appimagetool (packaging/config.env records both choices, PROPOSED). Written for Linux and NOT
# run here: this host is Windows. Checked with `bash -n` only; the first real run is on the Linux
# CI runner (todo O6). Packages are not signed: run tools/sign.sh minisign on the archives.
#
# The same app tree goes into every format, at these paths:
#   /usr/bin/<binary>, /usr/share/<binary>/app/, /usr/share/applications/<binary>.desktop,
#   /usr/share/doc/<binary>/compliance/ and licenses/
#
# usage: tools/bundle-linux.sh --binary <bin> --app-dir <dir> --version <v> --out <dir>
#          [--compliance <dir>] [--tgz] [--deb] [--rpm] [--appimage]
#   With no format flag, the tar.gz is written. --all is the same as --tgz --deb --rpm --appimage.
#
# Needs: tar and gzip (GNU tar; the mtime and owner flags are GNU-only), sed, and per format:
#   --deb, --rpm: nfpm (TPT_LINUX_PACKAGER)     --appimage: appimagetool (TPT_APPIMAGE_TOOL)

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
. "$here/packaging/config.env"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

binary=""; app_dir=""; version=""; out=""; compliance=""
want_tgz=0; want_deb=0; want_rpm=0; want_appimage=0
while [ $# -gt 0 ]; do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --app-dir) app_dir="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --compliance) compliance="$2"; shift 2 ;;
    --tgz) want_tgz=1; shift ;;
    --deb) want_deb=1; shift ;;
    --rpm) want_rpm=1; shift ;;
    --appimage) want_appimage=1; shift ;;
    --all) want_tgz=1; want_deb=1; want_rpm=1; want_appimage=1; shift ;;
    -h|--help) usage ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -n "$binary" ] && [ -n "$app_dir" ] && [ -n "$version" ] && [ -n "$out" ] || usage
if [ $((want_tgz + want_deb + want_rpm + want_appimage)) -eq 0 ]; then want_tgz=1; fi

[ -f "$binary" ] || { echo "missing binary: $binary" >&2; exit 1; }
[ -d "$app_dir" ] || { echo "missing app directory: $app_dir" >&2; exit 1; }

arch="x86_64"
stem="$APP_NAME-$version-linux-$arch"
work="$out/.work-$APP_BINARY"
rm -rf "$work"
doc="$work/root/usr/share/doc/$APP_BINARY"
mkdir -p "$out" "$work/root/usr/bin" "$work/root/usr/share/$APP_BINARY" \
  "$work/root/usr/share/applications" "$doc/licenses"
cp "$binary" "$work/root/usr/bin/$APP_BINARY"
chmod 755 "$work/root/usr/bin/$APP_BINARY"
cp -R "$app_dir" "$work/root/usr/share/$APP_BINARY/app"
[ -z "$compliance" ] || cp -R "$compliance" "$doc/compliance"
for licence in LICENSE-APACHE LICENSE-MIT; do
  [ -f "$here/$licence" ] && cp "$here/$licence" "$doc/licenses/$licence" || true
done
sed -e "s|@APP_NAME@|$APP_NAME|g" -e "s|@APP_BINARY@|$APP_BINARY|g" \
    -e "s|@APP_DESCRIPTION@|$APP_DESCRIPTION|g" \
    "$here/packaging/linux/app.desktop.in" > "$work/root/usr/share/applications/$APP_BINARY.desktop"

if [ "$want_tgz" -eq 1 ]; then
  # Owner 0, sorted names and a fixed mtime (SOURCE_DATE_EPOCH, or 0) keep the bytes stable.
  epoch="${SOURCE_DATE_EPOCH:-0}"
  tar -C "$work/root" --owner=0 --group=0 --numeric-owner --sort=name --mtime="@$epoch" \
    --transform "s,^\.,$stem," -cf - . | gzip -n > "$out/$stem.tar.gz"
  echo "tar.gz: $out/$stem.tar.gz"
fi

nfpm_package() {
  local packager="$1"
  command -v "$TPT_LINUX_PACKAGER" >/dev/null 2>&1 \
    || { echo "$TPT_LINUX_PACKAGER not found (needed for --$packager)" >&2; exit 1; }
  APP_VERSION="$version" \
  STAGE_BINARY="$work/root/usr/bin/$APP_BINARY" \
  STAGE_APP_DIR="$work/root/usr/share/$APP_BINARY/app" \
  STAGE_DESKTOP="$work/root/usr/share/applications/$APP_BINARY.desktop" \
  STAGE_DOC_DIR="$doc" \
  "$TPT_LINUX_PACKAGER" package --packager "$packager" \
    --config "$here/packaging/linux/nfpm.yaml" --target "$out"
}

if [ "$want_deb" -eq 1 ]; then nfpm_package deb; fi
if [ "$want_rpm" -eq 1 ]; then nfpm_package rpm; fi

if [ "$want_appimage" -eq 1 ]; then
  command -v "$TPT_APPIMAGE_TOOL" >/dev/null 2>&1 \
    || { echo "$TPT_APPIMAGE_TOOL not found (needed for --appimage)" >&2; exit 1; }
  appdir="$work/AppDir"
  rm -rf "$appdir"
  mkdir -p "$appdir/usr"
  cp -R "$work/root/usr/bin" "$appdir/usr/bin"
  cp -R "$work/root/usr/share" "$appdir/usr/share"
  sed "s|@APP_BINARY@|$APP_BINARY|g" "$here/packaging/linux/AppRun" > "$appdir/AppRun"
  chmod 755 "$appdir/AppRun"
  cp "$work/root/usr/share/applications/$APP_BINARY.desktop" "$appdir/$APP_BINARY.desktop"
  # The AppImage icon must be a PNG named after the binary. Supply it as ICON_PNG; the desktop
  # file names it through Icon=$APP_BINARY.
  [ -n "${ICON_PNG:-}" ] && [ -f "${ICON_PNG:-}" ] \
    || { echo "set ICON_PNG to a 256x256 PNG for the AppImage" >&2; exit 1; }
  cp "$ICON_PNG" "$appdir/$APP_BINARY.png"
  ARCH="$arch" "$TPT_APPIMAGE_TOOL" "$appdir" "$out/$stem.AppImage"
  echo "AppImage: $out/$stem.AppImage"
fi

rm -rf "$work"
(cd "$out" && for f in "$stem".* ; do
  case "$f" in *.tar.gz|*.deb|*.rpm|*.AppImage) sha256sum "$f" ;; esac
done > SHA256SUMS.linux)
echo "checksums: $out/SHA256SUMS.linux"
echo "not signed. Run tools/sign.sh minisign on the archives before distributing."
