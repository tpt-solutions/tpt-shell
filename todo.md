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
- [x] Initialise git repository
- [x] Add `LICENSE-MIT` (copyright TPT Solutions)
- [x] Add `LICENSE-APACHE` (Apache-2.0 text)
- [x] README with licence section ("MIT OR Apache-2.0", contribution clause)
- [x] SPDX header template and a CI check that enforces it
- [x] `THIRD_PARTY_NOTICES` skeleton (crates + system components: WebView2, WebKit, GTK)
- [x] Decide (O4): no crates.io publish now; publish at the M12 release. Name availability is not checked yet, so it is re-checked then
- [x] Reserve crate names: not needed while nothing is published (O4)
- [x] Scaffold workspace layout from §2 directories (`crates/`, `js/`, `templates/`, `examples/`, `editors/`, `docs/`, `fixtures/`, `conformance/`)
- [x] Facade crate `tpt-shell` compiles as an empty crate

### Dependency policy
- [x] Write `deny.toml`: licences allow-list, advisories, bans, sources
- [ ] Verify candidate crate licences with cargo-deny: serde, serde_json, sha2, ed25519-dalek (BSD-3-Clause), zstd, thiserror, clap, notify (CC0-1.0), tiny_http, windows-sys, objc2, libloading (ISC)
- [ ] For each non-MIT candidate: add deny.toml justification, or choose a hand-rolled fallback (e.g. own file polling instead of `notify`)
- [x] Confirm transitive dependency licences for every accepted crate (including "AND" expressions): `cargo deny check` passes on the current graph (2026-10-21)
- [x] Confirm no Apache-2.0-only crate in the graph (graph is empty today)
- [x] Done check: `cargo deny check` passes on the empty workspace (and still passes on the current workspace, 2026-10-21)

### CI skeleton (§15)
- [x] GitHub Actions matrix: windows-latest, macos-latest, ubuntu-latest (workflow written)
- [ ] Verify the workflow runs green on GitHub (not yet run)
- [ ] Add aarch64 runners where available
- [x] Jobs: fmt, clippy, tests, cargo-deny (and SPDX header check)
- [ ] Placeholder jobs: conformance, fuzz smoke, docs build, template build, visual artifact upload, benchmarks
- [ ] Linux headless display setup (Xvfb; Wayland headless where possible)
- [ ] Cache and MSRV job (MSRV job added: `cargo check --workspace --locked` on Rust 1.90.0; caching not done; the job has not run on GitHub yet)
- [x] Decide (O5): docs on GitHub Pages, built from the mdBook source in CI. Visual baselines in the repo via Git LFS, revisited if the store grows large
- [x] Decide (O6): CI runners only for now. Real hardware is acquired per phase when that phase's gate needs it (M4, M5, M7)

### Governance docs
- [x] SECURITY.md (private disclosure, response timeline)
- [x] CONTRIBUTING.md
- [x] CODE_OF_CONDUCT.md (contact placeholder to set)
- [x] Issue (bug report and feature request) and PR templates
- [x] RFC process doc
- [x] MSRV policy (Rust 1.90, measured on 1.90.0; set in `[workspace.package]`, described in CONTRIBUTING.md)
- [x] Semver and deprecation policy
- [x] CHANGELOG.md
- [x] Explicit no-telemetry statement in README and docs

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
- [ ] `WindowBackend` remaining: frameless, always-on-top, transparency, backdrop, drag regions, close handling (trait methods, fake backend and Windows backend done for frameless, always-on-top, backdrop and close action; transparency and drag regions still return `Unsupported` on Windows)
- [x] `WebviewBackend` trait, M1 subset: load html, navigate (app:// only), eval, post message, set_message_handler (page to shell; the default returns `Unsupported`, so macOS and Linux stay honest until they implement it; Windows and the fake implement it)
- [x] `WebviewBackend` crash callback and reload (verified by a renderer crash through DevTools), intercept (WebResourceRequested), nav policy hooks, doc-start script
- [ ] `WebviewBackend` remaining: snapshot, devtools toggle, profile binding, a11y tree (devtools and context menu are set per window at creation, release off, debug on; no runtime toggle is exposed yet)
- [ ] `Platform` trait (menus, tray, dialogs, clipboard, notifications, deep links, single instance, theme/accent, keystore) (clipboard text and colour scheme are in `tpt-shell-core::platform` with Windows and fake implementations; the rest are not started)
- [x] `Clock` and `Rng` traits (system and fake/seeded implementations, unit tested)
- [ ] Single UI thread model and `EventLoopProxy` (Send + Sync)
- [x] Minimal built-in async executor and worker pool (no external runtime): `block_on`, `Executor` (`execute`, `spawn`), tested
- [x] Fake backend `tpt-shell-core::fake`
- [ ] Unit tests for everything above the platform layer using the fake backend
- [x] `Error::Unsupported` and support-tag mechanism (`Support` enum)

### tpt-shell-ipc (raw)
- [x] Envelope v1: `{"v":1,"id","cmd","args"}`
- [x] Response, error and event shapes
- [x] Unknown version rejection with clear error
- [x] Limits: message size cap, depth cap and per-window rate limit done (configurable; `RateLimiter` token bucket with `Router::handle_limited`)
- [ ] Fuzz target: envelope parser (deterministic no-panic smoke tests exist; no cargo-fuzz target yet)

### Windows backend (tpt-shell-platform-windows)
- [x] Window class registration, `CreateWindowExW`, message loop (`WindowsBackend::run_until_quit`)
- [x] Per-monitor-v2 DPI and `WM_DPICHANGED` (the handler applies the suggested rectangle; no test changes the DPI yet)
- [x] RFC 0001 (WebView2 COM layer) approved with the recommended defaults, 2026-10-21 (docs/src/rfcs/0001-webview2-com-layer.md)
- [x] Default WebView2 user-data folder `%LOCALAPPDATA%\tpt-shell\<app-id>\webview2` (`default_user_data_dir`, RFC question 2; profile mapping stays in M6)
- [x] Runtime tests skip with a message when there is no interactive desktop (`runtime_test_loader`, RFC question 3)
- [x] Hand-written COM vtables (5 interfaces' used slots, header-checked) and ref-counted completion handlers (a function, not yet a general macro)
- [x] COM init on STA thread (`WindowsBackend::new`, balanced in `Drop`)
- [x] WebView2 Environment + Controller creation (verified against the installed runtime)
- [x] Navigate, navigate to string and execute script (`eval`; script errors are not reported yet)
- [x] Post web message / WebMessageReceived (page to Rust and Rust to page, verified; the backend exposes page messages through `set_message_handler`, and a runtime test runs a guarded command through it)
- [x] Add script on document created (`add_document_start_script`, verified by the shim test)
- [x] NavigationStarting policy: top-level navigations outside the app origin are cancelled, fail closed (verified). Frame navigations covered by set_frame_navigation_policy (verified with an iframe to a foreign origin)
- [x] WebResourceRequested + filters: works for a virtual https origin. DEVIATION: `app://` is not routed to WebResourceRequested on WebView2 (verified); pages use `https://tpt-app.localhost` on Windows. Decided: the virtual origin is accepted (RFC 0001 decision). Spec §5.4 and §9 wording not yet updated
- [x] `trait RuntimeLoader` (implemented by `OfficialLoader`; tested through a trait object)
- [ ] `RegistryLoader` (own runtime discovery): deferred. Registry key and install folder are confirmed, but the runtime lacks the environment entry point (see O1)
- [x] `OfficialLoader` (WebView2Loader.dll): loads the DLL, checks all three exports, reads the installed runtime version (verified 155.0.4283.62). Environment and controller creation still to do
- [x] Decide (O1): OfficialLoader. Licence is BSD-3-Clause (verified from the NuGet LICENSE.txt). RegistryLoader cannot work alone: the runtime lacks the environment entry point. Reversible if you prefer otherwise.
- [x] Callbacks marshalled onto UI thread (completion callbacks run from the UI thread's message pump)
- [x] Unit tests for the COM handlers (refcount, drop once, QueryInterface, owning pointer): done. Leak test: live callback objects return to baseline over 15 controller cycles; process handles stable (254 -> 255)
- [ ] Controller stall (not reproduced): one stall in a 60-cycle run (23rd controller, 30 s timeout) was not seen again in 30 shared-environment cycles or 10 fresh-environment cycles. Process count stayed flat (21-24). Cause unknown. Keep the timeout and watch in the 72 h soak (spec 10.1). Controller creation takes 0.7-5.8 s here, which needs a latency check against spec 13 (cold start)
- [ ] Cold start measured (warm runtime, 5 runs, separate processes): loader 0.5-0.7 ms, window 2-10 ms (shell budget 50 ms: met), environment about 6 ms, controller 0.8-2.0 s, first title 1.3-1.5 s. Total 2.1-2.3 s against the spec 13 budget of 500 ms: NOT MET. Budget: adopted as a reference-hardware target (see RFC 0001). Steady-state navigation is about 280 ms; first navigation after startup varies (0.6-2.1 s). True cold run not measured
- [ ] Startup overlap (async environment and controller while the app works): no reliable gain in 10 trials (median 2122 vs 2180 ms). Not claimed as a benefit

### Assets and protocol
- [x] `embed_dir!` prototype: `tpt-shell-macros`, plain bytes, sorted, content types. zstd compression and ETag still to do
- [x] Serving app pages from the virtual origin, from `embed_dir!` (verified with a subresource). DEVIATION from spec §5.4 `app://` Windows now answers every request under the app origin from the window's asset source (`set_asset_source`), with the page policy on each answer; `app_path` refuses `..` and backslashes. The facade serves `App::asset` files, and a runtime test shows a page's own script running at the app origin. The `embed_dir!` macro itself is still not wired to this
- [x] Never use `file://` (the navigation policy rejects it; the only `file://` strings are in tests)
- [x] JS runtime stub (`window.tpt.invoke`) injected at document start (`js/runtime/tpt-shell.js`, 661 bytes gzipped, verified end to end through the IPC router)

### Early baselines
- [ ] First benchmark: binary size, cold start, IPC round trip
- [ ] R1/R3 mitigations: strict trait boundary documented; leak test harness for COM objects
- [ ] Done: a Windows hello-world serves embedded assets and round-trips a message. Criterion met on Windows (embedded serving, shim round trip). Other M1 bullets are still open, so the phase stays open until you confirm

---

## Phase M2 — Typed contract, macros, codegen, manifest, capabilities, audit (basic chain)

### tpt-shell-macros
- [ ] `#[command]` with name, capability, kind (sync/async/job/stream), flags (ui, agent) (name, capability and kind = "sync" are done; a command with a capability registers behind its grant; `target(path = "arg")` (or device, url, bytes) names the argument the scope checks, and without it only an unrestricted grant allows the call; `target` without a capability is a compile error; async, job, stream and the ui and agent flags are not)
- [x] `commands![]` (and `register_commands!`, which registers the handlers on a router)
- [ ] `embed_dir!` (final) and `include_manifest!` (`embed_dir!` output reaches the app through `App::assets` and is served from the app origin; the directory is still read at compile time and does not rebuild on new files; `include_manifest!` is not built)
- [ ] `Contract` derive macro
- [ ] rust-analyzer friendliness checks
- [ ] Compile-fail tests (trybuild-style, licence-checked or hand-rolled) (one bad option was checked by hand; no test suite yet)

### Contract and validation (tpt-shell-ipc)
- [ ] Schema model: primitives with bounds, strings with max length/pattern, enums, structs, Option, Vec, string-key maps (all except regex patterns: strings use an allowed-character set instead)
- [ ] Scoped types: `ScopedPath`, `DeviceId`
- [x] Validator: unknown fields rejected, bounds checked, scope checked before handler runs (`Router::register_guarded` names a capability and a target; `Router::handle_authorized` checks the window's grant before the handler, and the unauthorized entry points refuse guarded commands. On Windows, page messages reach a handler set with `set_message_handler`, and a runtime test runs a guarded command through `handle_authorized`. App-level wiring exists in the facade (see the App builder item). macOS and Linux do not receive page messages yet)
- [x] `CmdError` with stable code, message, details (`CommandError::with_details` sends `error.details`; invalid args report the offending `path`)
- [ ] Blob channel `app://__tpt/blob/<id>` (store, quota, id check and route parsing are in `tpt-shell-ipc::blob`; the platform does not serve the route yet)
- [x] Stream channel with credit-based backpressure (`tpt-shell-ipc::stream`; not yet carried by a platform transport)
- [ ] Fuzz target: validator (no-panic smoke tests exist; no cargo-fuzz target yet)

### Codegen
- [x] `contract.json` writer (JSON Schema + metadata): `Contract::to_json_string`
- [x] TypeScript client generator (types, JSDoc, runtime bindings): `tpt-shell-ipc::typescript`; wrappers return `Promise<unknown>` because contracts have no result schema yet
- [x] Deterministic output
- [x] Golden tests for contract and TS generation (both have golden tests; the TS output also passes strict `tsc` in a one-off check)
- [x] JS runtime in `js/runtime/` (plain JS, <4 KB gzipped, size check in CI: `tools/check-runtime-size.sh` runs in the `runtime-size` job; currently 648 bytes gzipped; the job has not run on GitHub yet)
- [ ] `tpt.on()` events and `tpt.caps`

### tpt-shell-manifest
- [x] `tpt-shell.toml` parser with line-accurate errors (`TS0001`–`TS0004`)
- [x] Sections: app, windows, capabilities, kiosk, devices, audit, agent, replay, test, profiles, compliance (parse stubs for later phases)
- [x] Published JSON Schema for the manifest (`crates/tpt-shell-manifest/schema/`; a test checks its sections against the parser)
- [ ] Golden tests for manifest schema (consistency tests exist, now including app and window keys against the parser; no generated-schema golden yet, and the schema is hand-written)
- [ ] Fuzz target: manifest parser (no-panic smoke tests exist; no cargo-fuzz target yet)

### tpt-shell-capabilities
- [x] Permission engine, default deny (`tpt-shell-capabilities`)
- [x] Per-window and per-origin capabilities
- [ ] Scopes: path globs, device identifiers, URL patterns, size caps (path roots, device IDs, URL prefixes and size caps are done; path globs are not)
- [x] Native consent prompt abstraction (not web content): `ConsentPrompt` trait; the Windows implementation is a message box owned by the requesting window, default No (`tpt_shell_platform_windows::consent`). Its text is not localised yet, and macOS and Linux have no prompt
- [x] Remember-per-profile hook (storage lands in M6): `ConsentStore` trait, in-memory store for now
- [ ] Security tests: capability bypass, path-scope escapes (`..`, symlinks, UNC, case folding), oversized and malformed messages (`..`, UNC, device paths, ADS, reserved names, case folding, URL tricks, and size caps are tested; symlinks are not, since normalization is lexical)

### tpt-shell-audit (basic)
- [x] Entry model: seq, wall time, monotonic time, actor, command, capability, parameter hash, optional redacted params, outcome, duration
- [x] Canonical JSON for hashing
- [x] SHA-256 hash chain
- [ ] Redaction policy from manifest (`[audit] store_params` and `redact` parse into `AuditSettings`, validated with the schema; no runtime writer uses them yet, so the mapping to `tpt_shell_audit::Policy` is still to do)
- [x] Append-only file writer
- [ ] Property tests for chain integrity (randomized and exhaustive single-byte tests exist; not a proptest suite)
- [x] Golden tests for audit format

### Security defaults (§9)
- [ ] Only `app://` loaded; remote navigation blocked unless allowlisted per window (Windows enforces `[[windows]] allow_navigation` for top-level and frame navigations, with `https://` prefix matching at a host boundary, tested; the manifest value is parsed but no runtime reads it yet, since the shell does not create windows from the manifest; macOS and Linux not done)
- [x] Strict CSP injected on `app://` responses (Windows: every response from the virtual origin carries `PAGE_CSP`, no inline script or style, verified by a runtime test; the origin is `https://tpt-app.localhost`, see the DEVIATION note above; macOS and Linux not done)
- [ ] Devtools, context menu, view-source disabled in release (devtools and the default context menu are off in release builds on Windows, verified against WebView2 settings; view-source is not checked separately, and macOS and Linux are not done)
- [ ] Release-build CSP override needs an audited manifest flag
- [ ] Mock backend generated from contract (basic)
- [ ] Tauri-style invoke shim `tpt.compat.tauri` (documented subset)
- [ ] Done: generated TypeScript client calls validated commands; bypass attempts fail

---

## Phase M3 — CLI core, devtools overlay, theme injection, jobs API, error codes

### tpt-shell-cli
- [ ] `new` with vanilla and typescript templates (zero Node by default) (both are done; the TypeScript one needs `tsc` to build, which is the one place Node is used)
- [ ] `dev`: file watcher, hot reload preserving Rust state, mock toggle
- [ ] `dev`: automatic replay of last session after reload (hook; full replay in M8)
- [ ] Dev server (hand-rolled HTTP or licence-clean crate)
- [ ] `build` (release build; compliance outputs added in M10)
- [ ] `doctor` (WebView2 version, toolchain, licence policy): WebView2 version, rustc and cargo-deny presence are checked; the licence policy is not yet
- [ ] `contract gen | check | diff` (breaking-change detection)
- [ ] `manifest check | explain` (`manifest check` done; `explain` is `tpt-shell explain`, see below)
- [x] `explain <code>`
- [x] Error code registry `TS####` with explanations and fix hints (`tpt-shell-errors`)
- [x] Errors point at manifest lines (`tpt-shell manifest check` prints the line and the fix hint)
- [x] `cargo install tpt-shell-cli` works from a local path (checked with `--root` into the scratchpad)

### Jobs API
- [ ] `#[command(job)]` spawn, progress, cancel (runtime `tpt-shell-ipc::jobs` does spawn, progress, cancel and events; the `#[command(job)]` attribute is not built)
- [ ] Streamed results with backpressure (job results are one value; streams are a separate channel in `tpt-shell-ipc::stream`)
- [ ] Typed job events in TS client (`job.cancel()`)
- [ ] Optional progress component
- [ ] Tests with fake backend (jobs are tested directly on threads, not through the fake backend)

### Theming
- [x] Inject `--tpt-accent`, `--tpt-bg`, `--tpt-fg`, `--tpt-font` etc. (`tpt-shell-core::theme`, applied at document start on Windows; a change applies to windows created after `set_theme`)
- [ ] Keep `prefers-color-scheme` in sync (colour scheme is read when a window is created; it does not follow later changes yet)
- [ ] Signal high contrast, reduced motion, reduced transparency (the page has the attributes; Windows does not read the settings yet)
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
- [ ] Multiple windows, parent/child, modal, frameless, always-on-top, transparent (multiple windows, parent/child, modal, frameless and always-on-top work on Windows and are tested; transparency is still `Unsupported`)
- [ ] Dialogs (open, save, message, folder) via IFileDialog (open, save and folder dialogs are built with hand-written vtables checked against the SDK header; they are never shown in tests, and multi-select and the message box are not built)
- [x] App builder in the facade (`tpt_shell::App`): windows, grants and commands; each window's page messages go through the capability-checked router with that window's grants; a grant that needs consent asks in a native dialog owned by the window (default No); runtime tests run an app end to end, with the dialog answered Yes and No. Windows only, with `App::loader` set explicitly. Pages load from the app origin through `App::asset` (the old `Window::html` placed pages at a data: origin and is gone). Commands come from `#[command]` through `App::register`, and assets from `embed_dir!` through `App::assets`. Still missing: async handlers, jobs and streams, the worker pool, and `include_manifest!`
- [x] Clipboard (text first): `tpt-shell-platform-windows::clipboard`, restores the user's clipboard in its test
- [ ] Window state restore, fullscreen, multi-monitor

### Early measurements
- [ ] Time the three-command quickstart on a clean Windows machine
- [ ] Baseline benchmarks committed
- [x] Decide (O3): vanilla and TypeScript only for now. The other templates listed in M11 are added there
- [ ] Done: the three-command quickstart works on Windows in under five minutes

---

## Phase M4 — macOS backend and conformance parity with Windows

### Conformance suite foundation
- [ ] `conformance/` harness and fixture pages
- [ ] Checks: IPC round trip, large blobs, streams/backpressure, custom protocol, CSP, navigation policy, theme injection, snapshot, drag regions, DPI
- [ ] Run on Windows first, establish baseline
- [ ] Compatibility report generator (published artifact)

### macOS backend (tpt-shell-platform-macos)
- [x] Decide: objc2 (named in spec §6.2 as an allowed low-level FFI crate). Its licence is checked with cargo-deny when M4 adds it
- [ ] Own ObjC class/delegate declaration macro (R3)
- [ ] NSApplication run loop and EventLoopProxy
- [ ] NSWindow: bounds, title, fullscreen, frameless, always-on-top, transparency (create, show, hide, title, size, fullscreen and close are written against the generated bindings and type-check for macOS; they have not run on a Mac; frameless, always-on-top and transparency are not built)
- [ ] NSVisualEffectView vibrancy and appearance observation
- [ ] WKWebView with WKUserContentController script message handler (WKWebView is created and HTML is loaded; the message handler is not built)
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
- [x] Decide: real macOS hardware needs (O6), per the O6 decision above
- [ ] Done: macOS passes the conformance suite with parity to Windows

---

## Phase M5 — Linux backend and conformance parity

### Linux backend (tpt-shell-platform-linux)
- [x] dlopen loader via `libloading` with clear missing-library errors (`tpt-shell-platform-linux::loader`, tested with missing libraries)
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
- [x] Decide: real Linux hardware needs (O6), per the O6 decision above
- [ ] Done: Linux passes the conformance suite with parity (both adapters)

---

## Phase M6 — Profiles and ephemeral sessions (§10.7); kiosk runtime and supervisor (§10.1)

### tpt-shell-profiles
- [x] Profile kinds: default, named, ephemeral (`tpt-shell-profiles`)
- [ ] Window-to-profile binding
- [ ] Windows mapping: WebView2 profile name / user data folder
- [ ] macOS mapping: data-store identifier (14+), non-persistent otherwise
- [ ] Linux mapping: WebsiteDataManager base dirs / ephemeral
- [ ] `tpt.caps` reports profile capability level
- [ ] Wipe on close, wipe on idle (the wipe and idle policy exist in `tpt-shell-profiles`; no window calls them yet)
- [x] Secure erase (best-effort) with documented SSD/journaling limits (`wipe_dir` overwrites file contents before removal; the limits are in its doc comment)
- [x] Quotas (`usage` and `enforce_quota`; not yet applied to a live WebView2 profile)
- [ ] Profile manager API: create, list, switch, export (encrypted), delete (create, list and delete are done; switch and encrypted export are not)
- [ ] Per-profile storage of consent decisions
- [ ] Conformance: isolation (A writes invisible in B) and no-residue for ephemeral (file-level tests pass; no WebView2 storage conformance test yet)
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
- [ ] Restart on crash or hang (the crash decision and the hang check exist in `tpt-shell-supervisor`; no real process spawner or heartbeat channel yet)
- [x] Exponential backoff with crash-loop cap (`Backoff`, `CrashLoopGuard`)
- [ ] Alert hook (command or file) (a callback hook exists; command and file targets do not)
- [x] Scheduled restart (e.g. 03:00) (`Scheduled::is_due`, across midnight too; nothing calls it yet)
- [ ] Complete supervisor logs (`supervise` logs each restart and give-up; no file or rotation yet)
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
- [x] Device ID and scope types (vendor/product, port globs, host:port): `tpt-shell-devices::id`
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
- [x] ESC/POS builder (`tpt-shell-devices::escpos`, golden bytes)
- [x] ZPL builder (`tpt-shell-devices::zpl`, golden output and escaping)
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
- [x] Decide: visual baseline store implementation (O5): Git LFS in the repo

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
- [ ] `.tptreplay` format: zstd JSON Lines + assets + version (JSON Lines and version 1 are done; compression and assets are not)
- [x] `docs/replay-format.md` (format, readers, divergence and limits)
- [x] Golden tests for replay format
- [ ] Fuzz target: replay reader
- [ ] Player with strict and lenient modes, hidden window for CI (the strict and lenient readers exist; there is no player window)
- [x] Divergence reports (`tpt-shell-replay::divergence`, first difference by kind and data)
- [ ] `tpt-shell replay`
- [ ] `tpt-shell report` bug-report bundle with redaction hooks
- [ ] Documented determinism limits (R8) (limits are in `docs/replay-format.md`; the R8 review is not done)

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
- [ ] Ed25519 checkpoints every N entries or T seconds (entry-count interval done in `tpt-shell-audit`; the time interval is not, and the key comes from the caller, not a keystore)
- [ ] Keystore: DPAPI, Keychain, Secret Service
- [ ] HSM hook for key supply
- [ ] External anchor hook: file on other volume, syslog, HTTP
- [ ] Rotation and size limits
- [x] Export: JSON Lines, CSV (`tpt-shell audit export --format csv|jsonl`; both verify the chain first, and the CSV leaves out parameters and escapes formula-like cells)
- [x] `tpt-shell audit verify | export | view` (`verify --key <public-key-file>` checks checkpoint signatures; without a key it says they were not checked)
- [x] Verify reports first bad sequence number (`verify_file` and `verify_file_with_key`)
- [ ] Devtools audit viewer panel
- [ ] Tamper tests: flip, drop, swap, truncate all detected (flip, drop, swap, re-chain and forged signatures are tested; truncating the tail is a documented limit that needs an external anchor)
- [ ] Property tests for the full chain with checkpoints
- [ ] Document limits (compromised host, role of anchoring)
- [ ] `audit-app` template and `audit-viewer` example (preview)

### Agent (tpt-shell-agent)
- [ ] Cargo feature gate; off unless build and manifest enable an agent profile
- [ ] Semantic tree from DOM + ARIA: roles, names, states, values, bounds, snapshot-scoped refs
- [ ] Action API: click, type, select, scroll, focus, key, wait_for, read, invoke
- [x] No raw JavaScript evaluation (the agent crate has no evaluation path; tools are the only callable surface)
- [ ] `[agent]` profiles: actions, routes, rate limits, confirm-required commands, max session length (rate limits, confirm-required commands and max session length are in `tpt-shell-agent::Policy`; there are no manifest profiles or route rules yet)
- [ ] Redaction: password fields and `data-tpt-redact`
- [ ] Native confirmation dialogs for sensitive actions
- [ ] Kill switch (hotkey and API) (API: `Session::kill_switch`; no hotkey yet)
- [x] MCP server over stdio (`tpt-shell-agent::serve`: initialize, tools/list, tools/call; no resources yet)
- [ ] MCP server over localhost HTTP with token
- [ ] Tools generated from agent-flagged commands plus UI tools (tools are passed in explicitly; there is no agent flag on commands and no UI tools yet)
- [ ] Resources: semantic tree and audit tail
- [ ] Prompt-injection hygiene: untrusted markers, allowlist navigation (not started; tool results are not yet marked untrusted)
- [ ] All agent actions audited and recordable to replay (every call is logged in the session; nothing writes it to the audit file or replay yet)
- [ ] Policy tests: forbidden actions, rate limits, redaction, kill switch (forbidden actions, rate limits, confirmation, kill switch and session length are tested; redaction is not built)
- [ ] Fuzz and security tests on control channel (R7)
- [ ] `tpt-shell add agent`, `agent-demo` template and example
- [ ] Done: example agent completes a scripted task; policy tests pass; every action appears in the audit log

---

## Phase M10 — Compliance outputs, pinned runtime, vendoring, reproducible builds (§10.8); bundling and signing

### tpt-shell-compliance
- [x] CycloneDX 1.5 SBOM (`sbom.cdx.json`): `tpt-shell-compliance`, written by `tpt-shell compliance`; valid against the published 1.5 schema (`tools/validate-sbom.sh`)
- [x] SPDX 2.3 SBOM (`sbom.spdx.json`): same command; valid against the published 2.3 schema (`tools/validate-sbom.sh`)
- [ ] `licenses.html` and embedding at `app://__tpt/licenses`
- [ ] `THIRD_PARTY_NOTICES` generation
- [x] `attestation.json` (rustc, lockfile hash, output digests, policy result, git commit, build flags, reproducibility settings incl. SOURCE_DATE_EPOCH)
- [ ] Optional detached Ed25519 signatures
- [ ] Policy gate using the same cargo-deny logic as CI (the gate follows the `deny.toml` allow list with its own SPDX parser; it is not cargo-deny itself)
- [ ] SBOM schema validation tests (CycloneDX and SPDX) (`tools/validate-sbom.sh` runs both against the published schemas; not yet a CI job)
- [ ] Deliberately-bad-dependency test fails the build (the gate exits 1 on a forbidden licence and is unit-tested; nothing runs it from the build yet)

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
- [x] Decide (O2): external installer tool, not our own writer. The specific tool is chosen when bundle work starts
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
