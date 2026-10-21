# Agents

This guide is for developers who let an automated client (an AI agent or a script) drive an app's
pages, and for operators who decide whether to allow it. It explains what the agent can do, what
the policy stops, and what is not built.

Status: the agent crate (`tpt-shell-agent`) is built behind the code paths below. The session,
policy, tool allow-list, rate limit, route checks, confirmation, kill switch, redaction and MCP
server over stdio are tested. Action coverage on a real page is partial, and the manifest profiles
and several planned features are not finished. See the table at the end.

## The model

An agent talks to a session over JSON-RPC in the MCP style. Each message is one line, on standard
input and output. The session answers `initialize`, `tools/list`, `tools/call` and
`resources/read`.

The agent does not get JavaScript. It gets:

- **Tools**: the typed commands the app exposes to agents, one by one. Nothing else is callable.
- **Page actions** (click, type, select, scroll, focus, key, read) that the crate can run on a
  snapshot of the page. Not all of them are offered as tools yet.
- **Resources**: planned. The page snapshot exists, but it is not yet an MCP resource.

## What each call passes through

Every `tools/call` goes through these checks, in order. A refusal stops the call and is recorded.

1. The kill switch, and the session length limit (`max_session`, one hour by default).
2. Whether typed commands are allowed at all (`invoke`, on by default in the code; the manifest
   profile sets it).
3. The allow-list: only listed tools exist.
4. The rate limit (a burst and a per-second rate, per session).
5. Argument validation against the tool's schema.
6. The route allow-list, for any `url` or `route` argument. An empty list allows no route.
7. Confirmation, for tools the policy marks as needing it. The default confirmer refuses everything.
8. The handler.

Every call that reaches the agent is written to the session's action log, including refused calls.
If the recorder cannot write, the session stops, because an action that cannot be recorded must not
run (`TS0113`).

## Redaction

A password field and a block marked `data-tpt-redact` never reach a snapshot or a read. The
redaction is tested on a real page.

## Untrusted content

Text from the page is marked as untrusted (`<untrusted>` markers) in what the agent sees. That is a
convention. The model has to honour it; the session cannot enforce how the model reads it. Treat
anything an agent reads from a page as data, not as instructions.

## Profiles from the manifest

The `[agent]` section sets profiles. Nothing is enabled unless `enabled = true`:

```toml
[agent]
enabled = true

[agent.profiles.assistant]
actions = ["click", "type", "read", "invoke"]
routes = ["app://tpt-app.localhost/*"]
commands = ["set_volume"]
confirm = ["set_volume"]
rate_burst = 10
rate_per_second = 2
max_session_secs = 900
```

Unknown keys, unknown action names and unusable route patterns are refused with `TS0110`. A typo
cannot widen a profile. A route pattern is an exact path, or a prefix ending in `*`.

The action names are the verbs of the action API: `click`, `type`, `select`, `scroll`, `focus`,
`key`, `wait_for`, `read` and `invoke`. `invoke` runs typed commands. The code accepts `wait_for`
as a verb, but the todo does not count it as built, so do not enable it for a customer yet.

## Confirmation

Tools named in `confirm` need the user's approval before each call. The crate calls a `Confirm`
implementation, which the platform should back with a native dialog. Until that dialog exists, the
default `DenyAll` refuses every confirmation. Native dialogs for sensitive actions are not built
(todo M9).

## The MCP server

`tpt-shell-agent::serve` reads lines from standard input and writes one response per line. Use it
from an agent host that starts the app as a child process.

A localhost HTTP variant with a bearer token exists behind the `mcp-http` Cargo feature. It is off
by default because it opens a socket. The token must be at least 32 characters, and the address must
be 127.0.0.1 (`TS0111`). The todo does not count this as done, so treat it as unreviewed.

## Kill switch

`Session::kill_switch()` returns a shared flag. Setting it stops the session. There is no hotkey
yet. The operator's only control is the API or closing the app.

## Status

| Item | Status |
|---|---|
| Semantic snapshot of the page (roles, names, states, refs) | Built, tested on a real page |
| Stale references are refused | Built |
| Page actions: click, focus, type, select, key, scroll, read | Built on a real page |
| `wait_for` and `invoke` as page actions | Not counted as built (todo M9) |
| Raw JavaScript evaluation | Not offered. The agent has no path to it |
| Manifest `[agent]` profiles | Parsed and tested in the crate; the todo item for profiles is open |
| Tools from agent-flagged commands | Not built: tools are passed in explicitly |
| Resources (snapshot, audit tail) | Not built |
| Every action in the audit file | Not built. The agent keeps its own log |
| Action recording to replay | Not built |
| Native confirmation dialogs | Not built |
| Kill switch by hotkey | Not built (API only) |
| MCP over localhost HTTP | Behind `mcp-http`; not counted as done |
| Fuzzing of the control channel (R7) | Not done |
| `agent-demo` template and example | Not built |

Do not give an agent access to a production app until the audit log captures its actions and you
have tested your own confirmation flow.
