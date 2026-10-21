# Competitive scan

Status: **initial pass** (M0), with a first check against primary sources on 2026-10-22 (see "Primary-source check"). Entries not re-checked against primary sources are still from search-result snippets. Deepen it before any public claim (see "Open follow-ups"). Dates are the date of the search unless marked otherwise.

## Rules for claims (spec §1, R9)

- Do not write "first" or "only" anywhere in README, docs or marketing unless this file cites evidence for it.
- Use "known to offer" or "no mainstream Rust shell found in this scan" where the evidence is thin.
- Every entry below cites a source. An entry without a source must not be used in a claim.

## Licence checks (cargo info, 2026-10-20)

Registry licence fields, to be confirmed by `cargo deny` once a crate is added.

| Crate | Version seen | Licence field | Policy result |
|---|---|---|---|
| winit | 0.31.0-beta.3 | Apache-2.0 | Rejected (Apache-only). Matches spec §1. |
| tao | 0.37.1 | Apache-2.0 | Rejected (Apache-only). Matches spec §1. |
| wry | 0.57.0 | Apache-2.0 OR MIT | Accepted by policy. Not used: spec §1 builds the platform layer from scratch. |
| objc2 | 0.6.5 | MIT | Accepted. |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | Accepted (MIT branch). |
| tiny_http | 0.12.0 | MIT OR Apache-2.0 | Accepted (MIT branch). |
| webview2-com | 0.39.1 | MIT | Accepted. Reference only; not a dependency. |
| libloading | 0.9.0 | ISC | Needs a justification line in deny.toml (spec §4 allows ISC with a reason). |
| ed25519-dalek | 3.0.0 | BSD-3-Clause | Needs a justification line in deny.toml. |
| notify | 9.0.0-rc.6 | CC0-1.0 | Not on the spec's allow list. Use hand-rolled polling, or add a justified exception. |

Source: crate metadata via `cargo info` against the crates.io index (local run).

## Webview shells and frameworks

| Project | What it is | Relevance | Source |
|---|---|---|---|
| Wry | Cross-platform Rust webview library used by Tauri. WebView2, WebKit, WebKitGTK. Licence Apache-2.0 OR MIT (verified 2026-10-22 on docs.rs). | The main existing Rust webview layer. Confirms that no from-scratch platform layer is common. | [github.com/tauri-apps/wry](https://github.com/tauri-apps/wry); [docs.rs: wry WebContext](https://docs.rs/wry/latest/wry/struct.WebContext.html) |
| webview_app | Simple Rust wrapper: WebView2 and WebKitGTK 6.0. Serves embedded resources. | Small overlap with M1 (embedded assets + WebView2). Windows and Linux only. | [rust-digger: webview_app](https://rust-digger.code-maven.com/crates/webview_app) |
| Tauri v2 | Rust core, OS webview, IPC. | Closest mainstream comparison for the IPC and capability model. | [v2.tauri.app/security](https://v2.tauri.app/security/) |
| Electron | Bundles Chromium and Node. | Used as the kiosk comparison point. Ships its own engine, which spec §1 excludes. Has a kiosk mode (`BrowserWindow` `kiosk`, `setKiosk`, `isKiosk`). Licence MIT (verified 2026-10-22). | [electronjs.org: BrowserWindow](https://www.electronjs.org/docs/latest/api/browser-window); [electron LICENSE](https://github.com/electron/electron/blob/main/LICENSE) |
| Neutralinojs | Lightweight desktop framework: HTML, CSS and JS on the OS web renderer (GTK WebKit2 on Linux), with a WebSocket channel to native code and an embedded static server. No bundled browser engine. Licence MIT (verified 2026-10-22). | Closest in spirit to the shell's no-bundled-engine rule. Its native channel is a local socket, so its security model is the one to read before comparing. | [github.com/neutralinojs/neutralinojs](https://github.com/neutralinojs/neutralinojs) |
| Photino | .NET and C/C++ wrapper over WebView2, WKWebView and WebKitGTK. Licence Apache-2.0 per a directory listing (not yet read from the repository's LICENSE file). | Native-webview wrapper like the planned platform layer. Apache-only, so it cannot be reused under spec §4. | [tryphotino/photino.Native](https://github.com/tryphotino/photino.Native); [directory listing with licence](https://www.alternativeto.net/software/photino/about/) |

### Capability model (Tauri v2)

- Capabilities control which windows can reach which IPC commands. A window that matches no capability has no IPC access.
- Windows are matched by name or glob. Permissions can carry an allow-list scope (for example, one path).
- Source: [v2.tauri.app/reference/acl/capability](https://v2.tauri.app/reference/acl/capability).
- Verified 2026-10-22 on the Tauri v2 capabilities page: a capability lists target windows (for example `["main"]`, or `"*"`); the boundary follows window labels, not titles; remote URL access uses glob patterns such as `https://*.tauri.app`; by default every registered command is available to all windows, and `AppManifest::commands` in `build.rs` restricts that.
- Gap: that page does not say whether a permission can carry a path scope. Its scope answers point to separate Command Scopes and Asset protocol scope pages, which are not yet read.

## Kiosk and supervision

| Project | What it is | Relevance | Source |
|---|---|---|---|
| FreeKiosk | Android kiosk app on a WebView. Release notes describe random WebView reloads losing user input, and a lock-task call retried on focus. | Confirms that WebView reload and focus problems are real kiosk issues (spec §10.1 crash recovery). Not a Rust project. | [newreleases: FreeKiosk v1.2.20-beta.5](https://newreleases.io/project/github/RushB-fr/freekiosk/release/v1.2.20-beta.5) |
| Vigil | Rust software watchdog library. Callbacks on missed heartbeats. | Heartbeat idea is similar to the supervisor (spec §10.1). Library, not an app supervisor. | [docs.rs: vigil 0.2.0](https://docs.rs/crate/vigil/0.2.0) |

No Rust kiosk supervisor that combines a webview, watchdog, state restore and lock-down was found.

## Tamper-evident audit logs (spec §10.3)

| Project | What it is | Relevance | Source |
|---|---|---|---|
| secure-log | Rust. Hash-chained entries, Merkle-sealed segments, externally signed checkpoints. | Closest to §10.3. New project with no stars seen; maturity not checked. | [awesome.ecosyste.ms: secure-log](https://awesome.ecosyste.ms/projects/github.com%2Ftegmentum%2Fsecure-log) |
| decern-ledger | Rust. SHA-256 chain, Ed25519 signatures, offline verification. Notes that the chain alone cannot catch tail truncation, so the head needs external anchoring. | Matches §10.3 closely, including the truncation point. | [docs.rs: decern-ledger](https://docs.rs/decern-ledger) |
| metamorphic-log | Rust transparency-log SDK. Ed25519 checkpoints (C2SP signed note), inclusion and consistency proofs. Also post-quantum hybrid signing. | A reference for checkpoint format. Heavier than needed. | [metamorphic_log docs](https://metamorphic-log.hexdocs.pm/metamorphic_log.epub) |
| Provedex | Python library with a Rust signing core. Ed25519, SHA-256 chain, NDJSON. | Not a Rust crate. | [agentconn: Provedex](https://agentconn.com/agents/provedex/) |

Design note: spec §10.3 already states that a fully compromised host can delete everything, and that anchoring is the mitigation. decern-ledger's notes agree.

## Agent control and accessibility trees (spec §10.4)

| Project | What it is | Relevance | Source |
|---|---|---|---|
| OculOS | Rust daemon. Reads OS accessibility trees (UIA, AXUIElement, AT-SPI2), exposes elements as JSON and as an MCP server. | Closest to §10.4 for desktop apps in general. Reads other apps' trees, not a shell's own webview. | [Hacker News: OculOS](https://hn.svelte.dev/item/47285616) |
| agent-desktop | Rust CLI. Desktop automation through OS accessibility trees, deterministic element references, no screenshots. | Same idea as OculOS. | [feedbagel: agent-desktop](https://feedbagel.com/post/i-built-the-playwright-for-desktop-apps-80-token-savings) |
| computer-use-linux | Rust MCP server and CLI for Linux desktops, accessibility-first. | Linux only. | [computer-use-linux README](https://cdn.jsdelivr.net/gh/agent-sh/computer-use-linux@main/README.md) |

No project found that exposes a webview's own DOM and ARIA tree as an MCP server with an audited action API. The search did not confirm one either way. This is the most likely area for a real gap, so it needs a deeper search.

## Record, replay and time travel (spec §10.5)

| Project | What it is | Relevance | Source |
|---|---|---|---|
| Replay.io | Record and replay for web apps with browser dev tools. | Browser-focused. Not confirmed to work inside a Tauri or Electron window. | [nixpkgs: replay-io](https://mynixos.com/nixpkgs/package/replay-io) |
| Timelapse (research) | Extends WebKit to capture and replay a session exactly. | Relevant to the WebKit side of §10.5. Research code, not a product. | [UW CSE news](https://news.cs.washington.edu/?p=11114) |

No Tauri or Electron record-and-replay tool was found.

## Peripheral access (spec §10.2)

- Web Serial is not in Safari. MDN marks it not Baseline. It needs a secure context and a user gesture. Source: [MDN Web Serial API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API).
- HID devices use WebHID, not Web Serial.
- The Rust `serialport` crate covers serial. HID would need a separate crate. Source: [lib.rs: wterm](https://lib.rs/crates/wterm) (serial-to-WebSocket bridge, last release 2022).

## WebView2 loader licence (spec §4, O1)

- Packagers list the WebView2 loader as BSD-3-Clause: [MSYS2 package](https://packages.msys2.org/packages/mingw-w64-ucrt-x86_64-webview2-loader) and an [Eclipse IP review](https://gitlab.eclipse.org/eclipsefdn/emo-team/iplab/-/issues/14072) of the NuGet `WebView2Loader.dll`.
- The separate runtime [EULA](https://developer.microsoft.com/microsoft-edge/api/eula/webview2) has distribution conditions. It is not confirmed whether they cover the loader. A [forum thread](https://support.tmssoftware.com/t/webview2loader-x64-dll-webview2loader-x84-dll/26055) asks the same question and got no authoritative answer.
- Verified 2026-10-20: `LICENSE.txt` in the `Microsoft.Web.WebView2` 1.0.4312-prerelease NuGet package is the BSD-3-Clause text. The loader DLL exports `CreateCoreWebView2EnvironmentWithOptions`, `GetAvailableCoreWebView2BrowserVersionString` and `CreateCoreWebView2Environment`. The installed runtime (155.0.4283.62) does not contain the environment entry point, so the RegistryLoader route would need an undocumented runtime interface. Decision recorded in todo.md (O1): OfficialLoader.

## Summary

- The pieces exist separately: a Rust webview shell with typed IPC (Wry, Tauri), capability-scoped IPC (Tauri), and hash-chained audit logs with signed checkpoints (decern-ledger, secure-log). No single project was found combining them.
- No Rust kiosk supervisor that combines a webview, watchdog, state restore and lock-down was found.
- No Tauri or Electron record-and-replay tool was found.
- Agent control exists for other desktop apps through accessibility trees. Webview-internal control was not found.
- Overall: none of the eight differentiators is confirmed as unique. No claim of uniqueness is made.

## Primary-source check (2026-10-22)

Checked against the project's own pages: Tauri v2 capabilities (v2.tauri.app), Electron BrowserWindow and LICENSE (electronjs.org, github.com/electron), Wry WebContext (docs.rs), Neutralinojs README and licence line (github.com/neutralinojs). Photino: the repository URL returned 404 on fetch, so its licence is taken from a directory listing and is marked as such. Not checked in this pass: the code of any project, and the Electron and Tauri licences as stated in their package metadata.

## Open follow-ups

- [x] Read primary docs for Tauri capabilities, Electron kiosk and Wry data directory (2026-10-22; see "Primary-source check").
- [ ] Read Tauri Command Scopes and Asset protocol scope, and Tauri isolation.
- [ ] Read Electron session `partition` and the kiosk behaviour (the BrowserWindow page does not say what kiosk changes).
- [ ] Read Wry's profile handling for separate data stores per window (the WebContext page only suggests separate contexts).
- [ ] Read the Photino repository LICENSE file itself (the Apache-2.0 claim is from a directory listing; the repository URL returned 404 on the first fetch).
- [ ] Read the code and licence of decern-ledger and secure-log before any reuse decision. Reuse is not planned; this is for comparison.
- [ ] Search for webview-internal MCP or DOM-tree control projects.
- [ ] Search for Rust kiosk or digital-signage projects by name.
- [ ] Search for Rust HID crates and their licences (hidapi and others).
- [ ] Confirm the WebView2 loader licence from the NuGet package's LICENSE.txt (O1).
- [ ] Repeat the scan at M8 and M12 and update this file.
