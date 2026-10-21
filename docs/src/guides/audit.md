# Audit

This guide covers the audit log: what is written, how to check it, how to export it, and what it
cannot prove. It is for developers who turn it on, and for operators who check it.

Status: the append-only log with a SHA-256 hash chain, the CLI verify, export and view commands,
and Ed25519 checkpoints are built and tested. Each command call from a page is written when the app
is given an audit log. Anchoring to a second place and a keystore for signing keys are only partly
built (see the table at the end).

## What is written

Each record is one line of canonical JSON (sorted keys, no spaces). A record holds:

- `seq`: the sequence number, from 0;
- `wall_unix_ms` and `mono_ns`: wall-clock and monotonic times;
- `actor`: the window that made the call;
- `command` and `capability`: what was called and the capability it needed;
- `params_sha256`: the SHA-256 of the full parameters. This is always stored;
- `params`: a copy of the parameters, with the keys you name replaced by `[redacted]`. Only when
  you turn parameter storage on;
- `outcome`: `Ok`, or the stable error code (for example `CAP_NO_GRANT`);
- `duration_us`: how long the call took;
- `prev`: the hash of the record before it;
- `hash`: the SHA-256 of this record without its `hash` field.

The first record's `prev` is 64 zeros.

Checkpoints are records too. They name the sequence number and hash they cover, and a signature
over that hash, when signing is set up.

## Turning it on

From the manifest's `[audit]` section (the usual way):

```toml
[audit]
store_params = false
redact = ["password", "token"]
```

- `store_params = false` (the default) keeps only the hash. Set it to `true` only when you need the
  contents for an investigation, and say so in your privacy notes.
- `redact` lists object keys whose values are replaced at any depth, when parameters are stored.

In code, `App::manifest_audit(manifest, log)` reads that section and writes calls to `log`. The
manifest must name the `[audit]` section; a missing section keeps only the hashes. The facade
does not re-export the policy type, so `App::audit(path, policy)` needs the `tpt-shell-audit`
crate. Use `manifest_audit` unless you have a reason not to.

```rust,compile-check
use tpt_shell::App;

fn audited_app() -> tpt_shell::Result<App> {
    App::new()
        .name("audit-demo")
        .manifest_audit(std::path::Path::new("tpt-shell.toml"), "audit.jsonl")
}
```

The `[audit]` section is read at build time only. A running app does not reread it.

## Checking the log

```sh
tpt-shell audit verify audit.jsonl
tpt-shell audit verify audit.jsonl --key audit-public.key
tpt-shell audit export audit.jsonl --format csv
tpt-shell audit export audit.jsonl --format jsonl
tpt-shell audit view audit.jsonl
```

- `verify` recomputes the chain and reports the first bad sequence number. With `--key` it also
  checks checkpoint signatures. Without a key it says the signatures were not checked.
- `export` verifies the chain first, then writes CSV or JSON Lines. The CSV leaves out parameters
  and escapes cells that look like formulas.
- `view` prints the records for reading.

Exit codes follow the CLI: 0 for a good log, 1 when the checked input has problems, 2 for bad use
or a read failure.

## Signed checkpoints

`Signing` in `tpt-shell-audit` signs a checkpoint every N entries, or every T seconds. The time check
runs when an entry is written, so a quiet log gets its next checkpoint with the next entry. The key
comes from the caller: a key file (`FileKeySupplier`), a keystore (`StoreKeySupplier`), or an HSM
hook (`CheckpointSigner`). On Windows the keystore uses DPAPI, one file per name, per user.

Keep the signing key off the machine that writes the log, if you can. A key on the same machine can
be used by anyone who controls that machine.

## What the chain proves

The chain detects:

- a changed record;
- a removed record in the middle;
- records in the wrong order;
- a record that was re-chained after an edit, when signed checkpoints are on (the signature no
  longer matches).

The chain does **not** detect:

- removal of records from the end of the file. Nothing after them names what is missing;
- a whole file replaced by a new, valid chain;
- a compromised host, which can write any log it likes.

The mitigation for the first two is an external anchor: a copy of the chain head kept somewhere the
host cannot quietly rewrite. The anchor code exists (a file on another volume, and syslog lines),
and `TS0102` to `TS0104` cover its errors. It is not yet wired into the app's log, so do not rely on
it in production (todo M9).

## Rotation

`RotatingLog` writes numbered segments and keeps the chain across them. `verify_rotated` checks the
set, and a missing or out-of-order segment is `TS0104`.

## Status

| Item | Status |
|---|---|
| Hash chain, canonical JSON, golden format tests | Built, tested |
| Tamper tests: edit, drop, swap, re-chain, forged signature | Built, tested. Tail truncation is a documented limit |
| CLI verify, export (CSV, JSON Lines), view | Built, tested |
| Ed25519 checkpoints, verify with key | Built, tested |
| Keystore: DPAPI on Windows | Built. Keychain and Secret Service are not built |
| External anchors (file, syslog, HTTP) | File and syslog code exists, not wired to the app. HTTP not built |
| Rotation and size limits | Built |
| Devtools audit viewer | Not built (todo M9) |
| Agent actions in the audit log | Not yet (the agent keeps its own action log) |
| Limits document for compromised hosts | Partly, this page; the todo item is open |
