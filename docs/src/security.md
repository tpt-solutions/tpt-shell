# Security guide

This guide says what tpt-shell protects against, what it does by default, and what you must do
yourself. It is for app developers and the people who deploy apps. Each default below says whether
it is built. Where it is not, the todo item is named.

To report a vulnerability, follow [SECURITY.md](../../SECURITY.md). Do not open a public issue.

## Threat model

Taken from the product spec, section 9:

- **Web content is untrusted.** Pages can be buggy, or hostile if a remote page or a plugin is
  loaded. Everything a page sends is checked before Rust acts on it.
- **The Rust backend is trusted.** It is the code you write. Its bugs are yours.
- **The user is trusted, but the machine may be shared.** A guest may use a kiosk or a shared
  terminal. The shell does not try to stop the machine's owner, who can read files and install
  software.
- **The operating system and its webview are outside the shell.** WebView2, WebKit and GTK bugs
  belong to their vendors. The shell documents mitigations where it can.

## The default posture

| Default | Built? |
|---|---|
| Pages are served from one app origin (`https://tpt-app.localhost` on Windows) | Yes, Windows |
| A strict Content-Security-Policy on every response: no inline script or style, no remote origins, no plugins, no frames | Yes, Windows (`PAGE_CSP`, tested) |
| Every command checks a capability; no grant means no call | Yes (`tpt-shell-capabilities`, `tpt-shell-ipc`) |
| Grants are per window and per origin; another window's grants do not apply | Yes (tested) |
| Scopes: directory roots, path globs, device ids, URL prefixes, size caps | Yes (engine); the device hub and URL checks are tested |
| Consent prompts are native dialogs, default answer No, remembered per window and capability | Yes on Windows (`FileConsent` in the data folder) |
| Arguments are size-capped, depth-capped and schema-checked before the handler runs | Yes (`IPC_*` errors) |
| Remote navigation blocked unless allowed per window | Partly. Windows enforces `allow_navigation`; the manifest value is not read yet (todo M1) |
| Devtools and the default context menu off in release builds | Yes on Windows. View-source not checked; macOS and Linux not done (todo §9) |
| No telemetry; no sockets unless a feature needs one | Yes (tests check that the default build opens no socket) |

The release-build CSP override that the spec describes, with an audited manifest flag, is not
built (todo §9).

## Capabilities

Every privileged command names a capability. The grant names the window, the origin, the scope and
whether consent is needed:

```rust
use tpt_shell::{Grant, Scope};

let grant = Grant {
    capability: "fs.read".into(),
    window: "main".into(),
    origin: "https://tpt-app.localhost".into(),
    scope: Scope::Paths(vec!["C:/Data/Reports".into()]),
    consent: false,
};
```

Rules to follow:

- Grant the smallest scope. Prefer `Paths` or `Devices` to `Unrestricted`.
- Ask for consent for anything that reads user files or sends data out.
- Do not put a broad grant on a window that loads remote content, because the origin then
  identifies that content.
- A command registered without a capability is refused on the authorized path with
  `CAP_NOT_DECLARED`. Give every privileged command a capability, and check each one in a test.

## Inputs from pages

The router refuses, before your code runs:

- envelopes with an unknown version (`IPC_UNSUPPORTED_VERSION`), an unknown field
  (`IPC_UNKNOWN_FIELD`), or an oversized or over-deep message (`IPC_TOO_LARGE`, `IPC_TOO_DEEP`);
- arguments that fail the schema (`IPC_INVALID_ARGS`, `IPC_WRONG_TYPE`);
- calls above the rate limit (`IPC_RATE_LIMITED`).

Your handler still has to check business rules: whether a file is the one the user meant, whether
an amount is in range. The schema checks types and bounds, not intent.

## Audit

Turn on the audit log for any command that changes state or reads sensitive data. Parameters are
hashed by default, so the log can show that a call happened without keeping its contents. Use
`store_params` and `redact` in the manifest's `[audit]` section only when you need the contents,
and list every key that holds a secret. See [Audit](guides/audit.md).

Limits: the chain shows edits and reordering. It does not show that the end of the log was
removed. Anchors and signed checkpoints are the mitigation, and they are on the audit guide.

## Secrets

- The shell has a keystore API on Windows (DPAPI, one file per name, per user). Keychain and
  Secret Service are not built (todo M9).
- Do not put secrets in the page's assets. Anyone who can open the window can read them.
- Do not put secrets in the audit log or in replay recordings. Redact them first.

## Control channels

The test driver and the agent are the two ways a program can drive the app's pages. Both are
attack surface (risk R7 in the spec).

- The test driver exists only in builds with the `test-driver` feature. It uses a directory with a
  per-run token, not a socket.
- The agent surface is off unless a manifest profile enables it. It has no way to run JavaScript.
  It is rate-limited, refuses routes outside its allow-list, and can require confirmation.
- The HTTP variant of the agent server is behind the `mcp-http` feature, which is off by default.
  It needs a token of at least 32 characters and binds to 127.0.0.1 only.

The fuzz targets for these channels are not complete (todo M12 and M9). Treat them as not yet
reviewed.

## Kiosks and devices

- A kiosk config applies only when its Ed25519 signature checks out. A `home` page outside the app
  origin is refused.
- Key-chord blocking on Windows is best effort. Ctrl+Alt+Del cannot be blocked from user space. Use
  OS lock-down (see [Kiosk deployment](guides/kiosk.md)).
- Device opens and writes are audited by byte count. Payloads are not logged. Scopes cannot open a
  device of another kind.

## Build and supply chain

- `cargo deny check` runs in CI with the licence and advisory policy in `deny.toml`.
- The compliance command writes an SBOM in CycloneDX and SPDX form, and an attestation. Signing of
  those outputs is not built (todo M10). See [Compliance](guides/compliance.md).
- Builds use `--locked`. Reproducible-build guidance is not written yet (todo M10).

## Reporting

Use [SECURITY.md](../../SECURITY.md). It names the response targets. Do not post exploit details in
public issues.
