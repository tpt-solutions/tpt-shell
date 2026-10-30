# tpt-shell-ipc

The IPC envelope (version 1), its parsing limits, the response and event builders, and the command router for tpt-shell.

Every message between a page and the Rust backend goes through this crate. It checks the envelope, applies size and depth limits, routes the command to its handler, and checks the caller's grants before a guarded command runs.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## The envelope

Messages are JSON.

```text
page -> Rust:  {"v":1,"id":7,"cmd":"name","args":{...}}
Rust -> page:  {"id":7,"ok":true,"data":...}
               {"id":7,"ok":false,"error":{"code":"TS....","message":"..."}}
events:        {"event":"name","data":...}
```

Unknown envelope versions are rejected. Unknown envelope fields are rejected. Parsing enforces three limits, all in `Limits`:

- `max_message_bytes`: the largest accepted message.
- `max_depth`: the deepest accepted JSON nesting.
- `max_cmd_len`: the longest accepted command name.

## What it provides

| Module | Contents |
|---|---|
| root | `parse_request`, `ok_response`, `err_response`, `parse_error_response` and `encode`. |
| `router` | `Router`, the command table. `Caller` identifies the window and origin. `Target` is what a command runs against. `CommandError` carries a `TS####` code and a message, and can carry structured details. |
| `contract` | The typed contract of each command: names, argument and result schemas, and the `ui` and `agent` flags. |
| `contract_diff` | Lists the changes between two contracts, marked as breaking or compatible. Used by `tpt-shell contract diff` and `contract check`. |
| `contract_load` | Reads a contract from a file. |
| `schema` | The argument schemas that the router validates against. |
| `typescript` | Generates the TypeScript client (`commands.ts`). |
| `jobs` | Long-running commands with progress, cancellation and events. Cancellation applies only to jobs the calling window started. |
| `stream` | Streamed results. |
| `blob` | Binary payloads passed by reference. |
| `rate_limit` | Per-caller rate limits. |
| `scoped` | `ScopedPath` and `DeviceId`, typed arguments that carry their scope. |
| `reflect` | Introspection of registered commands. |
| `executor` | `block_on`, re-exported from the root. |

## Guarded commands

A command can be registered behind a capability with `register_guarded`. `handle_authorized` checks the caller's window and origin against the grants before the handler runs, and asks for consent when a grant needs it. A guarded command is refused on the unauthorised entry point, so it cannot run by mistake.

`Router::capability_of(cmd)` returns the capability a guarded command needs. The audit record uses it.

## Usage

```rust
use tpt_shell_ipc::{parse_request, Limits};

let message = br#"{"v":1,"id":1,"cmd":"ping","args":{}}"#;
let parsed = parse_request(message, &Limits::default());
assert!(parsed.is_ok());
```

Most apps do not call the parser directly. The facade's `App::command` registers handlers, and the shell calls the router for each message.

## Limits and errors

Every error the router returns has a stable `TS####` code. Run `tpt-shell explain TS####` for the meaning and a fix hint. The codes are in `tpt-shell-errors`.

## Platform support

None. The crate has no platform code. Message delivery is done by the backend crates.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
