# Changelog

All notable changes to `tpt-shell-macros` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `embed_dir!`: embeds a directory at compile time with deterministic, sorted paths.
- `#[command]`, with `commands!` and `register_commands!`.
- `#[command(capability = "...", target(...))]`: a command registers behind its grant, and the grant's scope checks the named argument (path, device, URL or bytes).
- `#[command]` supports async commands, job commands with progress and cancellation, and the `ui` and `agent` flags.
- `#[derive(Contract)]`, which derives the contract for command types.
- `include_manifest!`, which embeds the app's manifest.
- Compile-fail tests for command misuse.
