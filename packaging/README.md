<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- Copyright (c) 2026 TPT Solutions -->

# Packaging

Configuration and templates for bundling, signing, reproducible builds and release. Nothing here
publishes, uploads, submits or signs by itself. The scripts that do the work are in `tools/`.

## Layout

| Path | What it is | Status |
| --- | --- | --- |
| `config.env` | Shared defaults: app names, minimum OS versions, the chosen external tools | Written; sourced by the scripts |
| `windows/installer.iss` | Inno Setup 6 script for the installer (decision O2) | Written; not built |
| `macos/Info.plist.in` | Info.plist template for the .app | Written; not built |
| `linux/nfpm.yaml`, `linux/app.desktop.in`, `linux/AppRun` | .deb, .rpm and AppImage inputs | Written; not built |
| `runtime/webview2-fixed.example.toml` | Fields of a pinned WebView2 Fixed Version record | Written; the crate does not read it yet |
| `repro/REPRODUCIBLE.md`, `repro/cargo-config.toml` | Reproducible-build guidance and build config | Written; check not run |
| `procurement/PROCUREMENT-PACK.md.tmpl` | Procurement pack, filled by `tools/procurement-pack.py` | Written; filled once from test outputs |
| `dist/dist-workspace.toml` | cargo-dist configuration (copy of the repo-root file; keep them equal) | Copied to the root; `dist plan` passes with cargo-dist 0.32.0; workflow generated; no tag pushed |
| `winget/`, `scoop/`, `homebrew/` | Manifest templates | Written; not submitted |
| `release/RELEASE-CHECKLIST.md` | Release and v1.0.0 tag checklist | Written; no tag made |
| `release/RELEASE-NOTES.md.tmpl` | Release notes template | Written |

## Scripts

| Script | Purpose |
| --- | --- |
| `tools/bundle-windows.sh` | Portable folder, zip, and optional installer through the external tool |
| `tools/bundle-macos.sh` | .app, tar.gz and optional .dmg (run on macOS) |
| `tools/bundle-linux.sh` | tar.gz, .deb, .rpm (nfpm) and AppImage (run on Linux) |
| `tools/sign.sh` | Signing hooks: Windows, macOS notarisation, minisign. Each runs only with credentials |
| `tools/runtime-pin.sh` | `pin` and `list` for the WebView2 Fixed Version runtime (developer machine) |
| `tools/vendor.sh` | `cargo vendor --locked`, a record, and the source-replacement snippet |
| `tools/docs-offline.sh` | Offline docs bundle from mdBook, checked by `check-docs-offline.sh` |
| `tools/repro-build.sh`, `tools/repro-check.sh` | One reproducible build; two builds compared |
| `tools/release-attest.sh` | SHA256SUMS, release-attestation.json, compliance outputs, minisign if keyed |
| `tools/procurement-pack.py` | Fills the procurement pack from the compliance outputs |
| `tools/zip-dir.py` | Deterministic zip used by the bundle and docs scripts |

## Decisions recorded here

- **O2, external installer tool.** Windows installer: Inno Setup 6 (`ISCC.exe`). Linux packages:
  nfpm for .deb and .rpm, appimagetool for the AppImage. macOS .dmg: `hdiutil`, part of macOS.
  These are proposals in `config.env`; the owner confirms them.
- **O4, no crates.io publish at M12.** The release checklist re-checks the crate names.
- **O5, docs on GitHub Pages**, built from the mdBook source in CI.
- **O6, CI runners only** for macOS and Linux packaging until hardware is acquired.

## Trade-off: minisign keys in CI

`minisign -S` asks for the key password on a terminal, so a CI job cannot sign with a
password-protected key. Two options: (a) keep the signing key offline and sign on a person's
machine, which fits a manual release; (b) store a key without a password as a CI secret, which
lets the job sign but means the key is only as safe as the secret store. The release checklist
assumes (a) until the owner decides.

## Not verified

Nothing here has been run on macOS or Linux. The Windows zip and the procurement pack were run on
Windows with test outputs. The installer, the packages and the AppImage have not been built.
