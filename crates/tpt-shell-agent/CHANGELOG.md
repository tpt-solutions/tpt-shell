# Changelog

All notable changes to `tpt-shell-agent` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- MCP-style JSON-RPC session over newline-delimited messages on stdio.
- `[agent]` profile parsing. Invoking a command is gated by the profile's `invoke` action.
- The eight-step policy chain: kill switch, allow-list, rate limit, argument validation, route allow-list, confirmation and recording.
- Page accessible tree as a snapshot: roles, names, states, values, bounds and references.
- Actions `click`, `focus`, `type`, `select`, `key`, `scroll` and `read`. Password values and `data-tpt-redact` blocks are never reported.
- Resources for the semantic tree and the audit tail.
- Action log, including refused calls. A recorder failure stops the session.
- Page text is marked as untrusted content.
- `mcp-http` feature: MCP over `127.0.0.1` with a bearer token. Off by default.
- Error codes `TS0112` (route) and `TS0114` (invoke).
