#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 TPT Solutions
#
# Fills packaging/procurement/PROCUREMENT-PACK.md.tmpl from the compliance outputs (spec §10.8, todo
# "Procurement pack template"). Stdlib only. Run it as:
#
#   python3 -I tools/procurement-pack.py <compliance-dir> <output.md> [--release <release-dir>]
#                                        [--template <template.md.tmpl>]
#
# <compliance-dir> holds sbom.cdx.json and attestation.json, as written by `tpt-shell compliance`.
# --release names a directory with release-attestation.json (tools/release-attest.sh), whose
# artifact digests fill section 6. Without it, section 6 says that no release was given.
#
# The script fills what the outputs say. The questionnaire answers and the placeholders in
# [brackets] are left for a person to complete; the script never writes an answer that the
# repository does not support.

import datetime
import json
import os
import sys

DEFAULT_TEMPLATE = os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "..",
    "packaging",
    "procurement",
    "PROCUREMENT-PACK.md.tmpl",
)


def read_json(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def cell(text):
    return str(text).replace("|", "\\|").replace("\n", " ")


def component_rows(sbom):
    rows = ["| Component | Version | Licence |", "| --- | --- | --- |"]
    components = sorted(
        sbom.get("components", []),
        key=lambda c: (c.get("name", ""), c.get("version", "")),
    )
    for item in components:
        licences = item.get("licenses") or []
        expressions = [entry.get("expression") or entry.get("license", {}).get("id", "")
                       for entry in licences]
        licence = " ; ".join(e for e in expressions if e) or "[not recorded]"
        rows.append(f"| {cell(item.get('name', ''))} | {cell(item.get('version', ''))} | {cell(licence)} |")
    return rows, len(components)


def licence_rows(sbom):
    counts = {}
    for item in sbom.get("components", []):
        expressions = [entry.get("expression") or entry.get("license", {}).get("id", "")
                       for entry in item.get("licenses") or []]
        key = " ; ".join(e for e in expressions if e) or "[not recorded]"
        counts[key] = counts.get(key, 0) + 1
    rows = ["| Licence expression | Components |", "| --- | --- |"]
    for key in sorted(counts):
        rows.append(f"| {cell(key)} | {counts[key]} |")
    return rows


def artifact_rows(release_dir):
    if release_dir is None:
        return ["No release directory was given to the script. Run tools/release-attest.sh first."]
    record_path = os.path.join(release_dir, "release-attestation.json")
    if not os.path.isfile(record_path):
        return [f"No release-attestation.json in {release_dir}."]
    record = read_json(record_path)
    rows = ["| Artifact | SHA-256 |", "| --- | --- |"]
    for name in sorted(record.get("artifacts", {})):
        rows.append(f"| {cell(name)} | `{record['artifacts'][name]}` |")
    return rows


def main(argv):
    args = list(argv[1:])
    release = None
    template = DEFAULT_TEMPLATE
    if "--release" in args:
        i = args.index("--release")
        release = args[i + 1]
        del args[i:i + 2]
    if "--template" in args:
        i = args.index("--template")
        template = args[i + 1]
        del args[i:i + 2]
    if len(args) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    compliance_dir, output = args

    sbom = read_json(os.path.join(compliance_dir, "sbom.cdx.json"))
    attestation = read_json(os.path.join(compliance_dir, "attestation.json"))
    app = attestation.get("app", {})
    policy = attestation.get("policy", {})
    components, count = component_rows(sbom)

    values = {
        "APP_NAME": app.get("name", "[app name]"),
        "VERSION": app.get("version", "[version]"),
        "GENERATED_UTC": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
        "GIT_COMMIT": attestation.get("git_commit") or "[not recorded]",
        "RUSTC": attestation.get("rustc") or "[not recorded]",
        "LOCK_SHA256": attestation.get("lockfile_sha256") or "[not recorded]",
        "POLICY_RESULT": policy.get("result", "[not recorded]"),
        "COMPONENT_COUNT": str(count),
        "COMPONENT_TABLE": "\n".join(components),
        "LICENCE_TABLE": "\n".join(licence_rows(sbom)),
        "ARTIFACT_TABLE": "\n".join(artifact_rows(release)),
    }

    with open(template, encoding="utf-8") as handle:
        text = handle.read()
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    leftover = [k for k in values if "{{" + k + "}}" in text]
    if leftover:
        print(f"unfilled placeholders: {leftover}", file=sys.stderr)
        return 1

    with open(output, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)
    print(f"wrote {output} ({count} components)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
