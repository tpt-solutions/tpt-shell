<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- Copyright (c) 2026 TPT Solutions -->

# Release checklist (v1.0.0 and later)

This is a checklist, not a script. Every step that publishes, uploads, submits, signs or tags
is done by a person, after the earlier steps have passed. Nothing in the repository runs these
steps by itself. Tick a box only when the evidence is recorded (the log, the link, or the file).

## A. Before the release candidate

- [ ] `todo.md` Phases M10, M11 and M12 are ticked, or each open item is listed in the release
      notes as a known gap with its reason
- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`, `cargo deny check` pass on the release commit (CONTRIBUTING.md)
- [ ] Conformance suite green on Windows, macOS and Linux CI; compatibility report published
- [ ] `tools/repro-check.sh` passes on CI, or its differences are explained in
      `packaging/repro/REPRODUCIBLE.md` and in the procurement pack
- [ ] `tools/validate-sbom.sh` passes on the release's compliance outputs
- [ ] Benchmarks in `BENCHMARKS.md` updated for this commit; budget gaps listed (spec §13)
- [ ] CHANGELOG.md has the release section; "Unreleased" is empty
- [ ] Security review items (todo M12 "Fuzzing and security", "Risk review") are closed or listed
- [ ] `tools/docs-offline.sh` builds the offline docs; `tools/check-docs-offline.sh` passes
- [ ] The procurement pack is generated from this commit's compliance outputs and reviewed

## B. Build the artifacts (on CI, from the release commit)

- [ ] cargo-dist plan (`dist plan`) reviewed; `dist-workspace.toml` copied to the repo root
      (packaging/dist/dist-workspace.toml) and its cargo-dist version checked
- [ ] Release build of the CLI with `--locked` (`tools/repro-build.sh`)
- [ ] Windows: `tools/bundle-windows.sh --installer` (portable folder, zip, installer)
- [ ] macOS: `tools/bundle-macos.sh --dmg` on a macOS runner
- [ ] Linux: `tools/bundle-linux.sh --all` on a Linux runner (tar.gz, .deb, .rpm, AppImage)
- [ ] Runtime: `tools/runtime-pin.sh list` shows the pinned WebView2 runtime, if the release ships one

## C. Sign (only with credentials from the release environment)

- [ ] `tools/sign.sh check` lists the tools and credentials present
- [ ] `tools/sign.sh windows` on the binary and the installer; `signtool verify` passes
- [ ] `tools/sign.sh macos` on the .app, then notarisation; `stapler validate` passes
- [ ] `tools/sign.sh minisign` on the archives, SHA256SUMS and the SBOMs; verified with the public key
- [ ] Each signed artifact is checked again after signing (the digests in step D change)

## D. SBOM and attestation for the release

- [ ] `tools/release-attest.sh <release-dir> <version>` writes `SHA256SUMS`, `release-attestation.json`
      and the compliance outputs; the digests match the files
- [ ] Build provenance attestation for every artifact is made by the CI release workflow
      (`github-attestations = true` in dist-workspace.toml, or an attest step with
      `actions/attest-build-provenance`), and `gh attestation verify` passes for one artifact
- [ ] `release-notes` written from `packaging/release/RELEASE-NOTES.md.tmpl`

## E. Package manifests (written by hand, submitted by hand)

- [ ] winget: the three manifests from `packaging/winget/` filled with the release URL and the
      Windows zip's SHA-256; `winget validate` passes; submitted to microsoft/winget-pkgs by a person
- [ ] Scoop: `packaging/scoop/tpt-shell.json.tmpl` filled; `scoop install` tested on a clean machine
- [ ] Homebrew: `packaging/homebrew/tpt-shell.rb.tmpl` filled; `brew audit --strict` and
      `brew test` pass on macOS; the tap is updated by a person
- [ ] crates.io: not published at M12 (decision O4). Re-check the crate names at release time.

## F. Tag and publish (a person does this; nothing here is automated)

- [ ] Final review of the release notes and the procurement pack against the built artifacts
- [ ] Tag `v1.0.0` on the release commit (annotated, signed if a signing key exists)
- [ ] Push the tag; the release workflow builds from the tag only
- [ ] Attach the signed artifacts, SHA256SUMS, release-attestation.json and the SBOMs to the
      GitHub release; mark it as the latest release
- [ ] Publish the docs site (GitHub Pages, decision O5)
- [ ] Submit the winget, Scoop and Homebrew manifests (step E)

## G. After the release

- [ ] `todo.md`: the M12 release items ticked with links; the v1.0 definition of done (phase 13) checked
- [ ] Recheck `doctor` output on each OS against the minimum versions recorded in the release notes
- [ ] Open the first issue for each known gap listed in the release notes
