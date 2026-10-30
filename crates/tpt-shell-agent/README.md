# tpt-shell-agent

The agent surface for tpt-shell: a JSON-RPC session in the style of the Model Context Protocol (MCP), so an AI agent can read an app's accessible tree and call a limited set of its commands.

It runs over newline-delimited messages on stdio. With the `mcp-http` feature it also serves MCP on `127.0.0.1` behind a bearer token.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). Off by default. An app turns it on with the `agent` feature of the facade and an enabled `[agent]` profile.

## Safety model

An agent can do only what the policy allows. Nothing else is callable, and there is no path to evaluate JavaScript in the page.

Each `tools/call` and `resources/read` goes through these checks, in order:

1. The kill switch, and the session length limit.
2. Whether typed commands are allowed at all (the profile's `invoke` action).
3. The allow-list. Only the listed tools exist.
4. The rate limit, a `RateLimiter` per session.
5. Argument validation against the tool's schema.
6. The route allow-list, for any `url` or `route` argument. It fails closed when the list is empty.
7. Confirmation, for tools the policy marks as needing it.
8. The handler.

Every call that reaches the agent is recorded in the action log, including refused ones. If the recorder fails, the session stops, because an action that cannot be recorded must not run.

Results and resource text come from the page. They are wrapped and marked as untrusted content, so the model can tell page text from instructions. The marker is a convention for the model. The session cannot enforce how the model reads it.

## What an agent can read and do

The page's accessible tree is exposed as a snapshot: roles, names, states, values, bounds and references.

The actions are `click`, `focus`, `type`, `select`, `key`, `scroll` and `read`.

Password values and any block marked `data-tpt-redact` are never reported.

## Modules

| Module | Contents |
|---|---|
| `catalog` | The list of `AgentTool`s and how each one is described to the agent. |
| `page` | Snapshots of the page tree, and `mark_untrusted`. |
| `profile` | Builds the `Policy` from a manifest's `[agent]` profile. |
| `record` | `Recorder`, which writes the action log. |
| `resources` | The resources an agent can read: the semantic tree and the audit tail. |
| `http` | The localhost MCP server (feature `mcp-http`). |

Key types: `AgentTool`, `Policy`, `Outcome`, `ActionRecord`, `Session`, `Confirm` and `DenyAll`. `serve` runs a session over a reader and a writer.

Error codes: `TS0112` (route) and `TS0114` (invoke). Run `tpt-shell explain TS0112` for the fix hint.

## Features

| Feature | Default | What it enables |
|---|---|---|
| `mcp-http` | off | MCP over `127.0.0.1` with a bearer token. It opens a socket, so it is off by default. |

## Platform support

The protocol, policy and recording are platform-neutral and tested on every host. Native confirmation for agent actions is a Yes/No dialog on Windows, provided by `tpt-shell-platform-windows`.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
