# tpt-shell-audit

An append-only audit log for tpt-shell, with a SHA-256 hash chain. It makes edits, reordering and gaps in the log detectable. Signed checkpoints and external anchors close the remaining gap.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The chain, checkpoints, anchors and rotation are built. Platform keystores are wired on Windows only.

## How the log works

One line of the file is one record, written as canonical JSON: sorted keys and no whitespace.

- Each record stores the hash of the record before it (`prev`).
- Its own `hash` is SHA-256 over its canonical JSON without the `hash` field.
- The first record links to `GENESIS`, a string of zeros.

`verify_file` recomputes the chain and reports the first bad sequence number.

Each command call from a page is one record. It holds the window, the command, the capability used, and either the arguments or only their hash, and the outcome.

## What it can and cannot detect

The chain detects:

- edited records,
- reordered records,
- gaps in the sequence.

It cannot detect, on its own:

- removal of whole records from the end of the file,
- a rewritten file.

Two features close these gaps:

- **Signed checkpoints.** `Signing` writes an Ed25519-signed checkpoint after a set number of entries, or after a set number of seconds since the last one. A checkpoint covers everything before it, so removing records before it is detected. Checkpoints use the command name `tpt.checkpoint`.
- **External anchors.** `verify_file_anchored` checks the log against anchors kept outside the file: a file anchor, or a syslog-style anchor.

Size-based rotation keeps the chain continuous across segments. `verify_rotated` checks a set of rotated segments together. Truncation is detected when anchors are set.

## Usage

Verify a log:

```rust
use tpt_shell_audit::verify_file;

let summary = verify_file("audit.jsonl")?;
```

From the command line:

```sh
tpt-shell audit verify audit.jsonl --key public.key
tpt-shell audit verify audit.jsonl --anchors anchors.txt
tpt-shell audit export audit.jsonl --format csv
tpt-shell audit view audit.jsonl
```

In an app, `App::audit(path, policy)` writes the log. `Policy` decides what is recorded: the arguments, or only their hash, and which fields are redacted (`REDACTED` is the placeholder).

## Modules

| Module | Contents |
|---|---|
| root | `AuditLog`, `Entry`, `Policy`, `Outcome`, `verify_file`, `verify_file_with_key`, `verify_file_anchored`, `export_csv`, `canonical` and `sha256_hex`. |
| `keys` | The checkpoint signer, and the key supplier traits. |
| `anchor` | File and syslog-style anchors. |
| `segments` | `RotatingLog` and `verify_rotated`. |

## Platform support

The chain and verification are platform-neutral. The signing key is kept in the platform keystore: DPAPI on Windows. macOS and Linux keystores are written in their backends but not verified on a real machine.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
