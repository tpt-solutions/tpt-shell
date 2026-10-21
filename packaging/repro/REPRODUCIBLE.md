<!-- SPDX-License-Identifier: MIT OR Apache-2.0 -->
<!-- Copyright (c) 2026 TPT Solutions -->

# Reproducible builds

Spec §10.8 and the M10 acceptance rule: two builds of the same commit, on the same toolchain,
produce identical artifacts, or the difference is explained here. This page is the guidance; the
commands are in `tools/repro-build.sh` and `tools/repro-check.sh`.

## The rules

1. **Same commit.** Build from a clean checkout of one commit. The commit is recorded in
   `attestation.json` as `git_commit`.
2. **Same toolchain.** Use the Rust version the attestation records (`rustc`). The MSRV in
   CONTRIBUTING.md is a floor, not a pin.
3. **Locked dependencies.** Always `--locked`. Cargo then fails rather than changing `Cargo.lock`.
   `attestation.json` records the lock file hash and the flag.
4. **Fixed time.** `SOURCE_DATE_EPOCH` is the commit time (`git log -1 --format=%ct`). The
   compliance outputs and the release zip use it. The zip writer uses a fixed 1980 timestamp
   on every entry; tar uses `--mtime`.
5. **No absolute paths in the output.** `--remap-path-prefix` replaces the checkout, `CARGO_HOME`
   and the Rust sysroot with `/build/src`, `/build/cargo-home` and `/build/rust`.
6. **No incremental state.** `CARGO_INCREMENTAL=0`.
7. **Windows only:** `-C link-arg=/Brepro` gives the PE header a content hash instead of the link
   time. Without it, the PE timestamp changes every build.

Flags that are set by `tools/repro-build.sh`:

| Setting | Value | Why |
| --- | --- | --- |
| `SOURCE_DATE_EPOCH` | commit time | timestamps in outputs |
| `CARGO_INCREMENTAL` | `0` | no incremental state |
| `RUSTFLAGS` | `--remap-path-prefix` x3, and `-C link-arg=/Brepro` on MSVC | no local paths, fixed PE time |
| `cargo build` | `--release --locked` | fixed profile, fixed versions |

`packaging/repro/cargo-config.toml` is the project config to copy into an app's `.cargo/`
directory for the same settings when it builds with plain `cargo`.

## How to check

```sh
tools/repro-check.sh          # builds twice into target/repro/a and b, compares, exits 0 or 1
```

It compares the binary and every file of `tpt-shell compliance`. It does not run `cargo clean`.
Each build is a full build, so it is for CI, not for every commit. The CI job is listed in the
release checklist.

## Known differences

Nothing is known to differ yet: the check has not been run on CI. When it runs, record the result
here and in procurement pack section 2.13. Anything that differs must be listed with its cause:

| Output | Difference | Cause | Status |
| --- | --- | --- | --- |
| [none recorded] | | | |

Likely sources to look at first if the check fails: the PE header (check that `/Brepro` was
applied), a path that was not remapped (search the binary for the checkout path), a build script
that reads the time or the hostname, and a dependency that embeds a build date.

## Vendored builds

For an air-gapped build, run `tools/vendor.sh` first and build with the source replacement it
prints. Vendored sources come from the same lock file, so the result is the same as the online
build. The vendor record holds the lock hash and a digest over every crate's checksum file.
