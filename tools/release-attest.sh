#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# SBOM and attestation for a release (spec §16 M12; todo "SBOM and attestation for the release
# itself"). Given a directory that holds the release artifacts, it:
#   1. writes the SBOMs and attestation.json with `tpt-shell compliance <dir>/compliance`
#   2. writes SHA256SUMS for every artifact in the directory (sorted, so the file is stable)
#   3. writes release-attestation.json: version, git commit, tag, epoch, every artifact digest,
#      and the digest of the compliance attestation
#   4. signs SHA256SUMS, release-attestation.json and the SBOMs with minisign, only when
#      TPT_MINISIGN_KEY is set (tools/sign.sh minisign). Without the key it says so and goes on.
#
# Build provenance (the GitHub attestation of which workflow built each file) is made in CI by
# the attest step described in packaging/release/RELEASE-CHECKLIST.md. This script does not sign
# or publish anything by itself.
#
# usage: tools/release-attest.sh <release-directory> <version> [--tpt-shell <binary>]
#   <release-directory> must already hold the artifacts (zip, tar.gz, installers, .app, ...).
#   --tpt-shell defaults to the tpt-shell on PATH.

set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
. "$here/packaging/config.env"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

[ $# -ge 2 ] || usage
dist="$1"; version="$2"; shift 2
tpt="tpt-shell"
while [ $# -gt 0 ]; do
  case "$1" in
    --tpt-shell) tpt="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; usage ;;
  esac
done
[ -d "$dist" ] || { echo "no release directory: $dist" >&2; exit 1; }
dist="$(cd "$dist" && pwd)"

commit="$(git -C "$here" rev-parse HEAD 2>/dev/null || echo unknown)"
tag="$(git -C "$here" describe --tags --exact-match 2>/dev/null || echo none)"
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
  SOURCE_DATE_EPOCH="$(git -C "$here" log -1 --format=%ct HEAD 2>/dev/null || echo 0)"
fi
export SOURCE_DATE_EPOCH

# 1. Compliance outputs. A failing licence gate exits 1 but still writes the files.
mkdir -p "$dist/compliance"
(cd "$here" && "$tpt" compliance "$dist/compliance") || echo "note: licence gate reported a problem; see attestation.json" >&2
[ -f "$dist/compliance/attestation.json" ] || { echo "compliance outputs missing" >&2; exit 1; }

# 2. Checksums for the artifacts. The checksum files themselves are left out.
(cd "$dist" && find . -type f ! -name 'SHA256SUMS' ! -name 'release-attestation.json' \
  ! -name '*.minisig' -print | LC_ALL=C sort | sed 's#^\./##' | while IFS= read -r f; do
    sha256sum "$f"
  done) > "$dist/SHA256SUMS"

# 3. Release attestation. Python is used for the JSON, so the quoting is right.
"$PYTHON" -I - "$dist" "$version" "$commit" "$tag" "$SOURCE_DATE_EPOCH" <<'PY'
import hashlib, json, os, sys

dist, version, commit, tag, epoch = sys.argv[1:6]
artifacts = {}
with open(os.path.join(dist, "SHA256SUMS"), encoding="utf-8") as sums:
    for line in sums:
        parts = line.split(None, 1)
        if len(parts) == 2:
            artifacts[parts[1].strip().lstrip("*")] = parts[0]
with open(os.path.join(dist, "compliance", "attestation.json"), "rb") as f:
    compliance_digest = hashlib.sha256(f.read()).hexdigest()
record = {
    "schema": "tpt-shell-release-attestation/1",
    "version": version,
    "git_commit": commit,
    "tag": tag,
    "source_date_epoch": int(epoch),
    "artifacts": artifacts,
    "compliance_attestation_sha256": compliance_digest,
    "sboms": ["compliance/sbom.cdx.json", "compliance/sbom.spdx.json"],
    "signing": "minisign signatures (*.minisig) when a key was supplied; build provenance from CI",
}
with open(os.path.join(dist, "release-attestation.json"), "w", encoding="utf-8") as out:
    json.dump(record, out, indent=2, sort_keys=True)
    out.write("\n")
print(f"recorded {len(artifacts)} artifacts")
PY

# 4. Signatures, only when a key is supplied.
if [ -n "${TPT_MINISIGN_KEY:-}" ]; then
  "$here/tools/sign.sh" minisign \
    "$dist/SHA256SUMS" "$dist/release-attestation.json" \
    "$dist/compliance/sbom.cdx.json" "$dist/compliance/sbom.spdx.json"
else
  echo "minisign skipped: TPT_MINISIGN_KEY is not set" >&2
fi

echo "release directory: $dist"
echo "checksums: $dist/SHA256SUMS"
echo "attestation: $dist/release-attestation.json"
