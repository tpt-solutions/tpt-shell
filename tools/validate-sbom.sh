#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Validates the SBOMs written by `tpt-shell compliance <dir>` against the published schemas.
# Needs node and network access (downloads the schemas and ajv into a temporary directory).
#
# usage: tools/validate-sbom.sh <directory-with-sbom.cdx.json-and-sbom.spdx.json>

set -euo pipefail

dir="${1:?usage: validate-sbom.sh <directory>}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

base="https://raw.githubusercontent.com/CycloneDX/specification/master/schema"
curl -sSfL -o "$work/bom-1.5.schema.json" "$base/bom-1.5.schema.json"
curl -sSfL -o "$work/spdx.schema.json" "$base/spdx.schema.json"
curl -sSfL -o "$work/jsf-0.82.schema.json" "$base/jsf-0.82.schema.json"
curl -sSfL -o "$work/spdx-2.3.schema.json" \
  "https://raw.githubusercontent.com/spdx/spdx-spec/v2.3/schemas/spdx-schema.json"

ajv() {
  npx -y -p ajv-cli@5 -p ajv-formats@2 ajv "$@"
}

# --strict=false: ajv does not know the "iri-reference" and "idn-email" formats the
# CycloneDX schema uses. Those fields are checked only as strings.
ajv validate --spec=draft7 --strict=false -c ajv-formats \
  -s "$work/bom-1.5.schema.json" -r "$work/spdx.schema.json" -r "$work/jsf-0.82.schema.json" \
  -d "$dir/sbom.cdx.json"
ajv validate --spec=draft7 --strict=false -c ajv-formats \
  -s "$work/spdx-2.3.schema.json" -d "$dir/sbom.spdx.json"
