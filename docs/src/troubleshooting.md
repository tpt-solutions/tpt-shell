# Troubleshooting

Start with the code. Most problems print a stable code, and `tpt-shell explain <code>` prints its
meaning and a fix.

```sh
tpt-shell explain TS0002
```

Run `tpt-shell doctor` to check the tools on the machine. It reports the platform, `rustc` and
`cargo deny`, and other tools the build uses.

## Manifest and CLI codes

| Code | Title | What to do |
|---|---|---|
| TS0001 | manifest syntax error | Open the reported line and fix the TOML syntax or the value type |
| TS0002 | unknown manifest key | Usually a typo. Rename the key to the schema's spelling, or remove it |
| TS0003 | invalid manifest value | Use 1 to 64 ASCII letters, digits, `-` or `_` for names; positive sizes; unique window names; dotted lowercase capability names such as `fs.read` |
| TS0004 | missing manifest key | Add the section or key. Every manifest needs `[app]` with a name |

## Keys, audit and secrets

| Code | Title | What to do |
|---|---|---|
| TS0100 | signing key unavailable | Check that the key file exists and that the current user can read it, or that the keystore is unlocked |
| TS0101 | signature invalid | The file changed after signing, or the public key is not the one that signed it |
| TS0102 | anchor write failed | Check the anchor path, that it is writable, and that it is on the volume you meant |
| TS0103 | anchor mismatch or malformed anchor | The log may have been truncated, replaced or edited. Compare it with the anchor and restore from a backup if the anchor is the trusted copy |
| TS0104 | audit segments missing or out of order | Restore the missing segment from a backup or the anchor, then verify again |

## Agents, tests and licences

| Code | Title | What to do |
|---|---|---|
| TS0110 | agent profile invalid | Fix the reported key or route pattern in `[agent]`. Routes are exact paths, or a prefix ending in `*` |
| TS0111 | HTTP token or bind refused | Use a token of at least 32 characters, and bind to 127.0.0.1 only |
| TS0112 | route refused | Add the route to the profile if the agent should reach it |
| TS0113 | action not recorded, session stopped | Check that the audit log is writable and has room, then start a new session |
| TS0114 | invoke not allowed | Add the command to the profile's `invoke` list, or confirm when asked |
| TS0120 | deny.toml unreadable | Check that `deny.toml` is in the project root and is valid TOML with an allow list |
| TS0122 | licence gate refused | Replace the dependency, or record a reviewed exception in `deny.toml` |
| TS0301 to TS0305 | test driver and artefact errors | Use a documented driver command; check the app is running; use a plain relative artefact name |

The error list is in the `tpt-shell-errors` crate. The codes never change meaning once published.

## IPC and capability refusals

These come back from a page call. Your page should read `error.code`.

| Code | Usual cause | Fix |
|---|---|---|
| `CAP_NO_GRANT` | No grant names this capability, window and origin | Add the grant for that window and origin |
| `CAP_OUT_OF_SCOPE` | The target is outside the grant's scope | Widen the scope only if it is right, or fix the path the page sends |
| `CAP_BAD_REQUEST` | The command needs a target and the call had none | Send the argument the target function reads |
| `CAP_NOT_DECLARED` | The command has no capability | Give the command a capability |
| `CAP_CONSENT_DECLINED` | The user answered No to the consent dialog | Ask the user again later, or explain why the access is needed |
| `IPC_INVALID_ARGS` | Arguments fail the schema (wrong type, unknown field, out of bounds) | Match the schema the command declares |
| `IPC_UNKNOWN_COMMAND` | The command name is not registered | Check the spelling and the registration |
| `IPC_UNSUPPORTED_VERSION` | The envelope version is not 1 | Use the version the runtime sends |
| `IPC_RATE_LIMITED` | Too many calls in a short time | Slow the caller down |
| `IPC_TOO_LARGE`, `IPC_TOO_DEEP` | The message is over the size or depth limit | Send less data, or send it in pieces |

## Platform problems

- **The window does not open on Windows.** Check that `loader` points at the `WebView2Loader.dll`
  from the SDK package, and that the WebView2 runtime is installed. A second launch of a single
  instance exits at once, which is expected.
- **The app does not run on macOS or Linux.** `App::run` returns `Unsupported` there. This is
  expected until the backends are built. See [Platform notes](platform-notes.md).
- **The preview shows the page but commands do nothing.** A plain browser has no `window.tpt`.
  Run the app in a shell window.
- **Serial port refused.** Port discovery is not built, so name the port. Check that no other program
  has it open. Only Windows opens serial ports.
- **Printer times out or reports busy.** The device cannot keep up. Retries are built in; after
  them, `Busy` is returned. Send smaller jobs, or wait.
- **The scaffolded project opens no window.** That is expected. A Rust program has to build the app,
  as the hello example does. See the [Quickstart](quickstart.md).

## Replay and bug reports

- **Strict replay refuses a file.** Check the line number in the message. Strict mode refuses an
  unknown kind, a broken line, a gap in `seq`, or time going backwards. Use lenient mode only to look
  at the file.
- **A replay diverges.** The divergence report gives the first difference. Timing differences do not
  count. Cross-OS replay diverges by design. See the [Replay format](replay-format.md).

## Getting help

Use the issue templates for a bug or a feature. Include the code, the `tpt-shell doctor` output and
the platform. For a security problem, use [SECURITY.md](../../SECURITY.md).
