# Changelog

All notable changes to `tpt-shell-macros` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Changed
- The manifest points at this crate's README, `readme = "README.md"`.

### Added
- `embed_dir!`: embeds a directory at compile time with deterministic, sorted paths.
- `embed_dir_compressed!`: the same, with each file zstd-compressed at compile time (level 19). Pass the result to `App::assets_compressed`.
- `include_manifest!` is tested by expanding it on the vanilla template's manifest.
- `#[command]`, with `commands!` and `register_commands!`.
- `#[command(capability = "...", target(...))]`: a command registers behind its grant, and the grant's scope checks the named argument (path, device, URL or bytes).
- `#[command]` supports async commands, job commands with progress and cancellation, and the `ui` and `agent` flags.
- `#[derive(Contract)]`, which derives the contract for command types.
- `include_manifest!`, which embeds the app's manifest.
- Compile-fail tests for command misuse.
