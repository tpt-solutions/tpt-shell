# tpt-shell — Project Checklist

Owner: **TPT Solutions** · Licence: **MIT OR Apache-2.0** (dual, at the user's option) · Spec: [spec.txt](spec.txt)

How to use: tick `[x]` when done. `Decide:` items are open decisions from spec §19. `Done:` items are the milestone acceptance gate and are ticked last. Section references (§) point to spec.txt.

## Standing rules (apply to every phase)

- [ ] Every new dependency passes `cargo-deny` before merge (MIT, or dual-licensed with MIT; no Apache-2.0-only; no GPL/LGPL/MPL/AGPL)
- [ ] "A AND B" licence expressions count as requiring both (e.g. `dpi` is rejected)
- [ ] Permissive extras (BSD-3-Clause, Unicode-3.0, Unlicense, ISC, Zlib) carry a one-line justification in deny.toml
- [ ] Every public API item has a support tag: Full / Partial (gap stated) / Unsupported (`Error::Unsupported`)
- [ ] All `unsafe`/FFI stays in platform modules with their own tests
- [ ] Shell opens no sockets unless a feature is enabled
- [ ] No telemetry, anywhere
- [ ] No README claim says "first" or "only" without evidence in docs/competitive-scan.md
- [ ] Every error has a stable `TS####` code, explanation and fix hint
- [ ] Every source file carries an SPDX header `MIT OR Apache-2.0` and TPT Solutions copyright

## Crate index (crate → milestones that touch it)

| Crate | Milestones |
|---|---|
| tpt-shell (facade) | M0, M1, M2, M3, M11 |
| tpt-shell-core | M1, M3, M6 |
| tpt-shell-ipc | M1, M2, M3 |
| tpt-shell-macros | M2, M3 |
| tpt-shell-manifest | M2, M3, M6, M7, M9 |
| tpt-shell-capabilities | M2, M7, M9 |
| tpt-shell-audit | M2, M9 |
| tpt-shell-platform-windows | M1, M3, M6, M7, M8, M10 |
| tpt-shell-platform-macos | M4, M6, M7, M8 |
| tpt-shell-platform-linux | M5, M6, M7, M8 |
| tpt-shell-kiosk | M6 |
| tpt-shell-supervisor | M6 |
| tpt-shell-devices | M7 |
| tpt-shell-profiles | M6 |
| tpt-shell-agent | M9 |
| tpt-shell-replay | M8 |
| tpt-shell-test | M8 |
| tpt-shell-compliance | M10 |
| tpt-shell-devtools | M3, M8, M9 |
| tpt-shell-cli | M3, M6, M8, M9, M10, M11 |

---

## Phase M0 — Repo, policy, CI skeleton, docs skeleton, competitive scan

### Repo and licensing
- [x] Create root Cargo workspace (`[workspace]`, resolver 2)
- [ ] Initialise git repository (folder is not a git repo yet; needs your go-ahead)
- [x] Add `LICENSE-MIT` (copyright TPT Solutions)
- [x] Add `LICENSE-APACHE` (Apache-2.0 text)
- [x] README with licence section ("MIT OR Apache-2.0", contribution clause)
- [x] SPDX header template and a CI check that enforces it
- [x] `THIRD_PARTY_NOTICES` skeleton (crates + system components: WebView2, WebKit, GTK)
- [ ] Decide: crates.io publish under `tpt-shell-` prefix now? Check name availability (O4)
- [ ] Reserve crate names on crates.io if yes (O4)
- [x] Scaffold workspace layout from §2 directories (`crates/`, `js/`, `templates/`, `examples/`, `editors/`, `docs/`, `fixtures/`, `conformance/`)
- [x] Facade crate `tpt-shell` compiles as an empty crate

### Dependency policy
- [x] Write `deny.toml`: licences allow-list, advisories, bans, sources
- [ ] Verify candidate crate licences with cargo-deny: serde, serde_json, sha2, ed25519-dalek (BSD-3-Clause), zstd, thiserror, clap, notify (CC0-1.0), tiny_http, windows-sys, objc2, libloading (ISC)
- [ ] For each non-MIT candidate: add deny.toml justification, or choose a hand-rolled fallback (e.g. own file polling instead of `notify`)
- [ ] Confirm transitive dependency licences for every accepted crate (including "AND" expressions)
- [x] Confirm no Apache-2.0-only crate in the graph (graph is empty today)
- [x] Done check: `cargo deny check` passes on the empty workspace

### CI skeleton (§15)
- [x] GitHub Actions matrix: windows-latest, macos-latest, ubuntu-latest (workflow written)
- [ ] Verify the workflow runs green on GitHub (not yet run)
- [ ] Add aarch64 runners where available
- [x] Jobs: fmt, clippy, tests, cargo-deny (and SPDX header check)
- [ ] Placeholder jobs: conformance, fuzz smoke, docs build, template build, visual artifact upload, benchmarks
- [ ] Linux headless display setup (Xvfb; Wayland headless where possible)
- [ ] Cache and MSRV job
- [ ] Decide: hosting for docs and the visual-diff baseline store (O5)
- [ ] Decide: macOS and Linux test hardware beyond CI runners (O6)

### Governance docs
- [x] SECURITY.md (private disclosure, response timeline)
- [x] CONTRIBUTING.md
- [x] CODE_OF_CONDUCT.md (contact placeholder to set)
- [x] Issue (bug report) and PR templates (feature request still to add)
- [x] RFC process doc
- [ ] MSRV policy
- [x] Semver and deprecation policy
- [x] CHANGELOG.md
- [ ] Explicit no-telemetry statement in README and docs

### Docs skeleton
- [x] mdBook project in `docs/` with section stubs
- [ ] Offline-build of docs verified in CI
- [ ] Support-tag rendering approach decided
- [ ] Docs code-sample compile test harness

### Competitive scan (§16, R9)
- [x] Survey mainstream Rust webview shells and relevant non-Rust alternatives against the eight differentiators (initial pass)
- [ ] Deepen the scan from primary docs and code (follow-ups listed in docs/src/competitive-scan.md)
- [x] Record sources and dates in `docs/competitive-scan.md`
- [x] Define hedged wording rules for README claims
- [x] Done: cargo-deny passes on the empty workspace and the scan document exists with sources

---

## Phase M1 — Core traits, fake backend, Windows window + WebView2 + app:// + raw IPC

### tpt-shell-core
- [x] Own DPI and geometry types (no `dpi` crate)
- [x] `EventLoopBackend` trait: proxy, quit (M1 subset; `run` comes with the Windows event loop)
- [x] `WindowBackend` trait, M1 subset: create, show/hide, title, size, fullscreen, close
- [ ] `WindowBackend` remaining: frameless, always-on-top, transparency, backdrop, drag regions, close handling
- [x] `WebviewBackend` trait, M1 subset: load html, navigate (app:// only), eval, post message
- [ ] `WebviewBackend` remaining: intercept, nav policy hooks, crash callback, snapshot, devtools, profile, doc-start script, a11y tree
- [ ] `Platform` trait (menus, tray, dialogs, clipboard, notifications, deep links, single instance, theme/accent, keystore)
- [ ] `Clock` and `Rng` traits
- [ ] Single UI thread model and `EventLoopProxy` (Send + Sync)
- [ ] Minimal built-in async executor and worker pool (no external runtime)
- [x] Fake backend `tpt-shell-core::fake`
- [ ] Unit tests for everything above the platform layer using the fake backend
- [x] `Error::Unsupported` and support-tag mechanism (`Support` enum)

### tpt-shell-ipc (raw)
- [x] Envelope v1: `{"v":1,"id","cmd","args"}`
- [x] Response, error and event shapes
- [x] Unknown version rejection with clear error
- [ ] Limits: message size cap and depth cap done (configurable); per-window rate limit still to add
- [ ] Fuzz target: envelope parser

### Windows backend (tpt-shell-platform-windows)
- [ ] Window class registration, `CreateWindowExW`, message loop
- [ ] Per-monitor-v2 DPI and `WM_DPICHANGED`
- [ ] RFC 0001 (WebView2 COM layer) approved: proposed, awaiting your review (docs/src/rfcs/0001-webview2-com-layer.md)
- [x] Hand-written COM vtables (5 interfaces' used slots, header-checked) and ref-counted completion handlers (a function, not yet a general macro)
- [ ] COM init on STA thread: done in the runtime test; the app's own UI-thread init comes with the event loop
- [x] WebView2 Environment + Controller creation (verified against the installed runtime)
- [x] Navigate and navigate to string (execute script still to do)
- [x] Post web message / WebMessageReceived (page to Rust and Rust to page, verified)
- [x] Add script on document created (`add_document_start_script`, verified by the shim test)
- [x] NavigationStarting policy: top-level navigations outside the app origin are cancelled, fail closed (verified). Frame navigations still to do
- [x] WebResourceRequested + filters: works for a virtual https origin. DEVIATION: `app://` is not routed to WebResourceRequested on WebView2 (verified); pages use `https://tpt-app.localhost` on Windows. Needs your decision (see RFC 0001 status)
- [ ] `trait RuntimeLoader`
- [ ] `RegistryLoader` (own runtime discovery): deferred. Registry key and install folder are confirmed, but the runtime lacks the environment entry point (see O1)
- [x] `OfficialLoader` (WebView2Loader.dll): loads the DLL, checks all three exports, reads the installed runtime version (verified 155.0.4283.62). Environment and controller creation still to do
- [x] Decide (O1): OfficialLoader. Licence is BSD-3-Clause (verified from the NuGet LICENSE.txt). RegistryLoader cannot work alone: the runtime lacks the environment entry point. Reversible if you prefer otherwise.
- [x] Callbacks marshalled onto UI thread (completion callbacks run from the UI thread's message pump)
- [ ] Unit tests for the COM handlers (refcount, QueryInterface, leaks): not yet; the runtime test covers the happy path only

### Assets and protocol
- [ ] `embed_dir!` prototype (zstd compression, ETag)
- [x] Serving app pages from the virtual origin (inline bytes in the test; `embed_dir!` still to do). DEVIATION from spec §5.4 `app://`
- [ ] Never use `file://`
- [x] JS runtime stub (`window.tpt.invoke`) injected at document start (`js/runtime/tpt-shell.js`, 661 bytes gzipped, verified end to end through the IPC router)

### Early baselines
- [ ] First benchmark: binary size, cold start, IPC round trip
- [ ] R1/R3 mitigations: strict trait boundary documented; leak test harness for COM objects
- [ ] Done: a Windows hello-world serves embedded assets and round-trips a message. Round-trip and serving verified; `embed_dir!` and the JS shim are the remaining gaps

---

## Phase M2 — Typed contract, macros, codegen, manifest, capabilities, audit (basic chain)

### tpt-shell-macros
- [ ] `#[command]` with name, capability, kind (sync/async/job/stream), flags (ui, agent)
- [ ] `commands![]`
- [ ] `embed_dir!` (final) and `include_manifest!`
- [ ] `Contract` derive macro
- [ ] rust-analyzer friendliness checks
- [ ] Compile-fail tests (trybuild-style, licence-checked or hand-rolled)

### Contract and validation (tpt-shell-ipc)
- [ ] Schema model: primitives with bounds, strings with max length/pattern, enums, structs, Option, Vec, string-key maps
- [ ] Scoped types: `ScopedPath`, `DeviceId`
- [ ] Validator: unknown fields rejected, bounds checked, scope checked before handler runs
- [ ] `CmdError` with stable code, message, details
- [ ] Blob channel `app://__tpt/blob/<id>`
- [ ] Stream channel with credit-based backpressure
- [ ] Fuzz target: validator

### Codegen
- [ ] `contract.json` writer (JSON Schema + metadata)
- [ ] TypeScript client generator (types, JSDoc, runtime bindings)
- [ ] Deterministic output
- [ ] Golden tests for contract and TS generation
- [ ] JS runtime in `js/runtime/` (plain JS, <4 KB gzipped, size check in CI)
- [ ] `tpt.on()` events and `tpt.caps`

### tpt-shell-manifest
- [ ] `tpt-shell.toml` parser with line-accurate errors
- [ ] Sections: app, windows, capabilities, kiosk, devices, audit, agent, replay, test, profiles, compliance (parse stubs for later phases)
- [ ] Published JSON Schema for the manifest
- [ ] Golden tests for manifest schema
- [ ] Fuzz target: manifest parser

### tpt-shell-capabilities
- [ ] Permission engine, default deny
- [ ] Per-window and per-origin capabilities
- [ ] Scopes: path globs, device identifiers, URL patterns, size caps
- [ ] Native consent prompt abstraction (not web content)
- [ ] Remember-per-profile hook (storage lands in M6)
- [ ] Security tests: capability bypass, path-scope escapes (`..`, symlinks, UNC, case folding), oversized and malformed messages

### tpt-shell-audit (basic)
- [ ] Entry model: seq, wall time, monotonic time, actor, command, capability, parameter hash, optional redacted params, outcome, duration
- [ ] Canonical JSON for hashing
- [ ] SHA-256 hash chain
- [ ] Redaction policy from manifest
- [ ] Append-only file writer
- [ ] Property tests for chain integrity
- [ ] Golden tests for audit format

### Security defaults (§9)
- [ ] Only `app://` loaded; remote navigation blocked unless allowlisted per window
- [ ] Strict CSP injected on `app://` responses
- [ ] Devtools, context menu, view-source disabled in release
- [ ] Release-build CSP override needs an audited manifest flag
- [ ] Mock backend generated from contract (basic)
- [ ] Tauri-style invoke shim `tpt.compat.tauri` (documented subset)
- [ ] Done: generated TypeScript client calls validated commands; bypass attempts fail

---

## Phase M3 — CLI core, devtools overlay, theme injection, jobs API, error codes

### tpt-shell-cli
- [ ] `new` with vanilla and typescript templates (zero Node by default)
- [ ] `dev`: file watcher, hot reload preserving Rust state, mock toggle
- [ ] `dev`: automatic replay of last session after reload (hook; full replay in M8)
- [ ] Dev server (hand-rolled HTTP or licence-clean crate)
- [ ] `build` (release build; compliance outputs added in M10)
- [ ] `doctor` (WebView2 version, toolchain, licence policy)
- [ ] `contract gen | check | diff` (breaking-change detection)
- [ ] `manifest check | explain`
- [ ] `explain <code>`
- [ ] Error code registry `TS####` with explanations and fix hints
- [ ] Errors point at manifest lines or source spans
- [ ] `cargo install tpt-shell-cli` works from a local path

### Jobs API
- [ ] `#[command(job)]` spawn, progress, cancel
- [ ] Streamed results with backpressure
- [ ] Typed job events in TS client (`job.cancel()`)
- [ ] Optional progress component
- [ ] Tests with fake backend

### Theming
- [ ] Inject `--tpt-accent`, `--tpt-bg`, `--tpt-fg`, `--tpt-font` etc.
- [ ] Keep `prefers-color-scheme` in sync
- [ ] Signal high contrast, reduced motion, reduced transparency
- [ ] Windows Mica/Acrylic/Tabbed backdrop and dark title bar
- [ ] Normalisation shim (opt-in): viewport, scrollbar, font stack, drag region helper, safe-area insets

### Devtools overlay (tpt-shell-devtools, dev builds)
- [ ] IPC log with latency
- [ ] Jobs panel
- [ ] Capabilities in effect
- [ ] Audit tail
- [ ] Contract explorer (browse and call commands)
- [ ] Engine and caps info
- [ ] Toggle in dev builds only

### Windows baseline features (first pass)
- [ ] Multiple windows, parent/child, modal, frameless, always-on-top, transparent
- [ ] Dialogs (open, save, message, folder) via IFileDialog
- [ ] Clipboard (text first)
- [ ] Window state restore, fullscreen, multi-monitor

### Early measurements
- [ ] Time the three-command quickstart on a clean Windows machine
- [ ] Baseline benchmarks committed
- [ ] Decide: front-end templates to maintain beyond vanilla and TypeScript (O3, can defer to M11)
- [ ] Done: the three-command quickstart works on Windows in under five minutes

---

## Phase M4 — macOS backend and conformance parity with Windows

### Conformance suite foundation
- [ ] `conformance/` harness and fixture pages
- [ ] Checks: IPC round trip, large blobs, streams/backpressure, custom protocol, CSP, navigation policy, theme injection, snapshot, drag regions, DPI
- [ ] Run on Windows first, establish baseline
- [ ] Compatibility report generator (published artifact)

### macOS backend (tpt-shell-platform-macos)
- [ ] Decide: objc2 vs raw libobjc FFI (verify licence with cargo-deny)
- [ ] Own ObjC class/delegate declaration macro (R3)
- [ ] NSApplication run loop and EventLoopProxy
- [ ] NSWindow: bounds, title, fullscreen, frameless, always-on-top, transparency
- [ ] NSVisualEffectView vibrancy and appearance observation
- [ ] WKWebView with WKUserContentController script message handler
- [ ] WKUserScript at document start
- [ ] WKURLSchemeHandler for `app://`
- [ ] WKNavigationDelegate (navigation policy, crash callback)
- [ ] `takeSnapshot`
- [ ] NSOpenPanel / NSSavePanel
- [ ] NSPasteboard (text, HTML, image, files)
- [ ] UNUserNotificationCenter
- [ ] NSStatusItem tray and native menus
- [ ] Deep links via Apple Events
- [ ] Single instance via NSRunningApplication with arg forwarding
- [ ] Keystore (Keychain)
- [ ] NSAccessibility tree access
- [ ] Global hotkeys (Partial)
- [ ] Support tags: macOS 12+ matrix, profile limits on 12–13

### Parity
- [ ] Conformance green on macOS CI
- [ ] Unit and leak tests for ObjC wrappers
- [ ] Sanitiser run on macOS FFI module
- [ ] macOS notarisation notes in docs
- [ ] x86_64 and aarch64 builds verified
- [ ] Decide: real macOS hardware needs (O6)
- [ ] Done: macOS passes the conformance suite with parity to Windows

---

## Phase M5 — Linux backend and conformance parity

### Linux backend (tpt-shell-platform-linux)
- [ ] dlopen loader via `libloading` with clear missing-library errors
- [ ] GTK3 + WebKit2GTK 4.1 adapter
- [ ] GTK4 + WebKitGTK 6.0 adapter
- [ ] Runtime adapter selection and `doctor` check
- [ ] GTK main loop and EventLoopProxy
- [ ] Window features: bounds, title, fullscreen, frameless, always-on-top, transparency
- [ ] WebKitUserContentManager messages and scripts
- [ ] `webkit_web_context_register_uri_scheme` for `app://`
- [ ] Navigation policy and crash signals
- [ ] `webkit_web_view_get_snapshot`
- [ ] GtkFileChooser dialogs
- [ ] Clipboard (GtkClipboard / GdkClipboard)
- [ ] Tray (StatusNotifier over D-Bus; GtkStatusIcon fallback)
- [ ] Freedesktop notifications over D-Bus
- [ ] Single instance over UNIX socket
- [ ] Deep link handling (desktop file registration)
- [ ] Keystore (Secret Service)
- [ ] AT-SPI accessibility dump
- [ ] Global hotkeys X11-only (Partial tag)
- [ ] Wayland and X11 verification
- [ ] D-Bus client written without a non-compliant dependency, or licence-checked crate

### Parity
- [ ] Conformance green on Ubuntu CI (Xvfb, Wayland headless where possible)
- [ ] Both GTK3/4 adapters covered in CI matrix (R4)
- [ ] aarch64 Linux build verified
- [ ] Compatibility report covers all three OSes
- [ ] Decide: real Linux hardware needs (O6)
- [ ] Done: Linux passes the conformance suite with parity (both adapters)

---

## Phase M6 — Profiles and ephemeral sessions (§10.7); kiosk runtime and supervisor (§10.1)

### tpt-shell-profiles
- [ ] Profile kinds: default, named, ephemeral
- [ ] Window-to-profile binding
- [ ] Windows mapping: WebView2 profile name / user data folder
- [ ] macOS mapping: data-store identifier (14+), non-persistent otherwise
- [ ] Linux mapping: WebsiteDataManager base dirs / ephemeral
- [ ] `tpt.caps` reports profile capability level
- [ ] Wipe on close, wipe on idle
- [ ] Secure erase (best-effort) with documented SSD/journaling limits
- [ ] Quotas
- [ ] Profile manager API: create, list, switch, export (encrypted), delete
- [ ] Per-profile storage of consent decisions
- [ ] Conformance: isolation (A writes invisible in B) and no-residue for ephemeral
- [ ] Done: isolation and no-residue tests pass on all three OSes

### tpt-shell-kiosk
- [ ] `[kiosk] mode = "single-app"` full-screen, single window, allowlisted navigation
- [ ] Disable devtools, context menu, zoom, optional text selection, drag-drop, print shortcuts, function keys, swipe gestures
- [ ] Windows low-level keyboard hook (Alt+Tab, Win, Alt+F4, Ctrl+Esc), best effort
- [ ] Document OS lock-down recipes: Windows Assigned Access, macOS guided access/MDM, Linux dedicated session (R10)
- [ ] `app.snapshot_state()` on interval and on exit, typed state store, restore on restart
- [ ] Page crash recovery: reload to last good state
- [ ] Idle reset: home route, clear session, ephemeral integration
- [ ] Multi-monitor placement, rotation, per-monitor kiosk windows
- [ ] Signed local config (Ed25519)
- [ ] Local admin endpoint (off by default, token protected)
- [ ] Heartbeat file and optional local HTTP health endpoint (off by default)

### tpt-shell-supervisor (separate binary)
- [ ] Spawn app and watch heartbeat over local IPC
- [ ] Restart on crash or hang
- [ ] Exponential backoff with crash-loop cap
- [ ] Alert hook (command or file)
- [ ] Scheduled restart (e.g. 03:00)
- [ ] Complete supervisor logs
- [ ] Idle RAM ≤ 5 MB measured

### Boot integration and CLI
- [ ] `tpt-shell install-service`: Task Scheduler/Startup entry, LaunchAgent, systemd user unit
- [ ] `tpt-shell add kiosk`
- [ ] `kiosk` template (preview, finalised in M11)

### Soak
- [ ] Fault-injection harness: crashes, hangs, page failures
- [ ] 72-hour soak on each OS with memory growth checks
- [ ] Done: 72 h soak passes; app returns to configured state each time; supervisor logs complete

---

## Phase M7 — Device bridge, virtual devices, printers and scanners (§10.2)

### tpt-shell-devices core
- [ ] `Transport` trait
- [ ] Device ID and scope types (vendor/product, port globs, host:port)
- [ ] API: `devices.list(filter)`, `open(id, options)`, read/write streams with backpressure, control transfers
- [ ] Discovery and opt-in hotplug events

### Transports
- [ ] Serial: Win32 COM
- [ ] Serial: termios (macOS, Linux)
- [ ] HID: Windows HID
- [ ] HID: IOKit
- [ ] HID: hidraw (Linux, udev notes)
- [ ] USB raw: WinUSB
- [ ] USB raw: IOKit
- [ ] USB raw: usbfs
- [ ] TCP/UDP raw sockets (network printers)

### Helpers
- [ ] ESC/POS builder
- [ ] ZPL builder
- [ ] Receipt/label printing
- [ ] Raw print via OS spooler
- [ ] Barcode scanners: keyboard-wedge, serial, HID POS
- [ ] Scales: common serial protocols
- [ ] Customer displays

### Security and audit
- [ ] Capabilities `device.serial`, `device.hid`, `device.usb`, `device.net`, `device.print` with scopes
- [ ] Native consent dialog on first use, remembered per profile
- [ ] Audit every open, close, write
- [ ] Payload logging off by default; hash-only or bytes per manifest
- [ ] Security tests: scope escapes, bad device IDs

### Virtual devices
- [ ] VirtualSerial
- [ ] VirtualHid
- [ ] VirtualPrinter (ESC/POS receipt preview)
- [ ] VirtualScanner (scripted scans)
- [ ] Selection by manifest or env var

### Docs, tags, CLI
- [ ] Per-OS support tags (Windows Full, Linux Full, macOS Partial for reserved USB classes)
- [ ] `tpt-shell add devices`
- [ ] `devices-sim` example
- [ ] `pos` template (preview)
- [ ] Real-device matrix: one serial adapter, one HID scanner, one ESC/POS printer
- [ ] Done: virtual-device round-trip tests pass on all three OSes; real-device matrix documented

---

## Phase M8 — Test driver, a11y audit, visual diff (§10.6); record/replay and time-travel (§10.5)

### Test driver (tpt-shell-test)
- [ ] Cargo feature `test-driver`, per-run token, local IPC (named pipe / UNIX socket)
- [ ] Rust API: `find(role().name())`, click, type_text, press, screenshot, dom_snapshot, a11y_tree, eval (test builds only), wait_for
- [ ] Expectations with automatic retry
- [ ] Screenshots: CapturePreview, takeSnapshot, get_snapshot
- [ ] `tpt-shell test` with artifact collection
- [ ] Integration with virtual devices and replay
- [ ] Fuzz and security tests on control channel (R7)

### Visual diff
- [ ] Own perceptual comparer with region masks and per-region tolerance
- [ ] `tpt-shell visual compare` merging artifacts from three CI jobs
- [ ] HTML report
- [ ] Threshold failures
- [ ] WebView2 channels (stable/beta/canary) support on Windows
- [ ] Decide: visual baseline store implementation (O5)

### Accessibility audit
- [ ] Rule engine for WCAG 2.2 AA subset: names/roles, contrast, focus order, keyboard traps, target size, form labels, reduced motion
- [ ] Native tree dump: UIA
- [ ] Native tree dump: NSAccessibility
- [ ] Native tree dump: AT-SPI
- [ ] DOM-vs-native comparison report
- [ ] Seeded-violation fixture page
- [ ] Devtools a11y quick check

### Record/replay (tpt-shell-replay)
- [ ] Recorder: IPC calls with args/results/timing, DOM input events with stable paths, window events, protocol loads
- [ ] Virtualised Rust `Clock`/`Rng` in record/replay mode
- [ ] In-page shim for Date.now, performance.now, Math.random, timers (record/replay only)
- [ ] `.tptreplay` format: zstd JSON Lines + assets + version
- [ ] `docs/replay-format.md`
- [ ] Golden tests for replay format
- [ ] Fuzz target: replay reader
- [ ] Player with strict and lenient modes, hidden window for CI
- [ ] Divergence reports
- [ ] `tpt-shell replay`
- [ ] `tpt-shell report` bug-report bundle with redaction hooks
- [ ] Documented determinism limits (R8)

### Devtools time-travel
- [ ] Timeline scrubber, step forward/back
- [ ] Inspect IPC state and DOM snapshot at any event
- [ ] Jump to divergence
- [ ] `dev` auto-replays last session after reload (complete)

### Gates
- [ ] Same test suite green on all three CI OSes
- [ ] Visual-compare report produced from real artifacts
- [ ] A11y audit finds all seeded violations
- [ ] 100 consecutive strict replays identical on recording OS
- [ ] Cross-OS lenient replay reports differences
- [ ] Done: M8 acceptance criteria of §10.5 and §10.6 met

---

## Phase M9 — Agent surface and MCP server (§10.4); audit checkpoints, anchoring, verify (§10.3)

### Audit completion (tpt-shell-audit)
- [ ] Ed25519 checkpoints every N entries or T seconds
- [ ] Keystore: DPAPI, Keychain, Secret Service
- [ ] HSM hook for key supply
- [ ] External anchor hook: file on other volume, syslog, HTTP
- [ ] Rotation and size limits
- [ ] Export: JSON Lines, CSV
- [ ] `tpt-shell audit verify | export | view`
- [ ] Verify reports first bad sequence number
- [ ] Devtools audit viewer panel
- [ ] Tamper tests: flip, drop, swap, truncate all detected
- [ ] Property tests for the full chain with checkpoints
- [ ] Document limits (compromised host, role of anchoring)
- [ ] `audit-app` template and `audit-viewer` example (preview)

### Agent (tpt-shell-agent)
- [ ] Cargo feature gate; off unless build and manifest enable an agent profile
- [ ] Semantic tree from DOM + ARIA: roles, names, states, values, bounds, snapshot-scoped refs
- [ ] Action API: click, type, select, scroll, focus, key, wait_for, read, invoke
- [ ] No raw JavaScript evaluation
- [ ] `[agent]` profiles: actions, routes, rate limits, confirm-required commands, max session length
- [ ] Redaction: password fields and `data-tpt-redact`
- [ ] Native confirmation dialogs for sensitive actions
- [ ] Kill switch (hotkey and API)
- [ ] MCP server over stdio
- [ ] MCP server over localhost HTTP with token
- [ ] Tools generated from agent-flagged commands plus UI tools
- [ ] Resources: semantic tree and audit tail
- [ ] Prompt-injection hygiene: untrusted markers, allowlist navigation
- [ ] All agent actions audited and recordable to replay
- [ ] Policy tests: forbidden actions, rate limits, redaction, kill switch
- [ ] Fuzz and security tests on control channel (R7)
- [ ] `tpt-shell add agent`, `agent-demo` template and example
- [ ] Done: example agent completes a scripted task; policy tests pass; every action appears in the audit log

---

## Phase M10 — Compliance outputs, pinned runtime, vendoring, reproducible builds (§10.8); bundling and signing

### tpt-shell-compliance
- [ ] CycloneDX 1.5 SBOM (`sbom.cdx.json`)
- [ ] SPDX 2.3 SBOM (`sbom.spdx.json`)
- [ ] `licenses.html` and embedding at `app://__tpt/licenses`
- [ ] `THIRD_PARTY_NOTICES` generation
- [ ] `attestation.json` (rustc, lockfile hash, policy result, git commit, flags, reproducibility settings)
- [ ] Optional detached Ed25519 signatures
- [ ] Policy gate using the same cargo-deny logic as CI
- [ ] SBOM schema validation tests (CycloneDX and SPDX)
- [ ] Deliberately-bad-dependency test fails the build

### Runtime and air-gap
- [ ] `tpt-shell runtime pin | list` (WebView2 Fixed Version fetch + bundle)
- [ ] Shipped app never contacts Microsoft
- [ ] macOS/Linux minimum versions recorded; `doctor` checks them; limit documented
- [ ] Test: shell opens no sockets by default
- [ ] `tpt-shell vendor`
- [ ] `tpt-shell docs --offline`
- [ ] Reproducible-build guidance and flags (`--locked`, path remapping, `SOURCE_DATE_EPOCH`)
- [ ] Reproducibility test: two builds of same commit produce identical artifacts, or diff explained
- [ ] Procurement pack template (questionnaire, data-flow diagram, SBOM, licence summary, audit/kiosk explainer)

### Bundling and signing
- [ ] Decide: Windows installer technology, external tool vs own writer (O2)
- [ ] `bundle` Windows: portable folder, zip, installer
- [ ] `bundle` macOS: .app and .dmg
- [ ] `bundle` Linux: tar.gz, .deb, .rpm, AppImage
- [ ] `sign`: Windows code signing hooks
- [ ] `sign`: macOS notarisation
- [ ] `sign`: minisign for archives
- [ ] `doctor` checks signing tools
- [ ] Done: SBOMs validate; bad dependency fails build; reproducibility shown or explained

---

## Phase M11 — Docs site, templates, VS Code extension, migration shims, examples, rustvac port

### Templates
- [ ] Templates: vanilla, typescript, appfront-dom, kiosk, pos, agent-demo, audit-app
- [ ] Any extra front-end templates per decision (O3)
- [ ] CI: generate, build, smoke-test every template

### Examples
- [ ] kiosk
- [ ] pos
- [ ] audit-viewer
- [ ] agent-demo
- [ ] devices-sim
- [ ] rustvac port

### Docs site (mdBook)
- [ ] Five-minute quickstart
- [ ] Concepts
- [ ] Cookbook with 20+ recipes
- [ ] Guides: kiosk deployment, devices, audit, agents, profiles, compliance
- [ ] Security guide
- [ ] Platform notes and troubleshooting
- [ ] API reference with rendered support tags
- [ ] Migration guides: Tauri, Electron
- [ ] Examples gallery
- [ ] Published compatibility report
- [ ] Docs code samples compiled in CI
- [ ] Offline docs bundle verified
- [ ] Publish docs site (per O5)

### Editor and CI tooling
- [ ] VS Code extension: manifest schema and completion
- [ ] VS Code extension: contract explorer
- [ ] VS Code extension: run/debug configs
- [ ] VS Code extension: audit viewer
- [ ] VS Code extension: inline error codes
- [ ] Reusable GitHub Actions: build matrix, test, visual compare, compliance gate, release
- [ ] Migration shims: finalise `tpt.compat.tauri`
- [ ] `tpt-shell add` for kiosk, devices, audit, agent

### rustvac (§20)
- [ ] rustvac-gui rewritten as HTML UI on tpt-shell
- [ ] Scan uses the jobs API
- [ ] Compress and trash use capabilities and audit entries
- [ ] Triage rustvac bug reports back into earlier phases
- [ ] Done: all docs, templates, extension, shims, examples built and tested; rustvac runs on tpt-shell

---

## Phase M12 — Hardening and v1.0.0

### Fuzzing and security
- [ ] Fuzz envelope parser, validator, manifest parser, audit chain, replay reader (extended runs)
- [ ] Security test pass: capability bypass, oversized/malformed messages, path-scope escapes
- [ ] External or internal security review
- [ ] Control-channel review (R7)
- [ ] Soundness review of all unsafe code (R1, R3)
- [ ] Sanitiser and Miri runs where possible

### Performance (§13)
- [ ] Hello-world binary ≤ 4 MB stripped on Windows
- [ ] Cold start to first paint ≤ 500 ms; shell overhead ≤ 50 ms
- [ ] IPC round trip small payload p95 ≤ 5 ms
- [ ] Blob channel ≥ 50 MB/s sustained
- [ ] Idle shell memory overhead ≤ 25 MB
- [ ] Supervisor ≤ 5 MB idle
- [ ] `BENCHMARKS.md` with methodology and results
- [ ] Document any budget gaps

### Risk review
- [ ] R1 three backends: parity confirmed
- [ ] R2 loader licence: O1 decision documented
- [ ] R3 COM/ObjC leaks: soak and sanitiser results
- [ ] R4 WebKitGTK drift: both adapters green
- [ ] R5 sprawl: all support tags accurate
- [ ] R6 device differences: real-device matrix current
- [ ] R7 control channels: gated, tokenised, fuzzed
- [ ] R8 replay limits: documented
- [ ] R9 competitive claims: README audited against evidence
- [ ] R10 kiosk lock-down: recipes and wording reviewed

### Release engineering
- [ ] cargo-dist binaries
- [ ] Publish crates to crates.io (per O4)
- [ ] winget manifest
- [ ] Scoop manifest
- [ ] Homebrew formula
- [ ] Release notes
- [ ] SBOM and attestation for the release itself
- [ ] Tag `v1.0.0`
- [ ] Done: all M12 gates complete and release published

---

## Phase 13 — v1.0 definition of done (§17)

- [ ] All eight differentiators implemented, documented and tested on Windows, macOS and Linux with accurate per-OS support tags
- [ ] cargo-deny passes: licences, advisories, bans, sources
- [ ] Three-command quickstart works on all three OSes
- [ ] Conformance suite green on all three CI OSes; compatibility report published
- [ ] 72 h kiosk soak passed
- [ ] Audit tamper tests pass
- [ ] Replay determinism test passes
- [ ] Benchmarks published; §13 budgets met or gaps documented
- [ ] Docs site, templates, VS Code extension, CI actions and migration guides released
- [ ] rustvac runs on tpt-shell as its UI
- [ ] v1.0.0 tag, release notes, SBOM and attestation exist for the release itself
- [ ] All open items O1–O6 resolved and recorded
