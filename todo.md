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
- [x] Verify candidate crate licences with cargo-deny: serde, serde_json, sha2, ed25519-dalek (BSD-3-Clause), zstd, thiserror, clap, notify (CC0-1.0), tiny_http, windows-sys, objc2, libloading (ISC)
- [ ] For each non-MIT candidate: add deny.toml justification, or choose a hand-rolled fallback (e.g. own file polling instead of `notify`)
- [x] Confirm transitive dependency licences for every accepted crate (including "AND" expressions): `cargo deny check` passes on the current graph (2026-10-21)
- [x] Confirm no Apache-2.0-only crate in the graph (graph is empty today)
- [x] Done check: `cargo deny check` passes on the empty workspace (and still passes on the current workspace, 2026-10-21)

### CI skeleton (§15)
- [x] GitHub Actions matrix: windows-latest, macos-latest, ubuntu-latest (workflow written)
- [x] Verify the workflow runs green on GitHub (run 11, commit 3eef0cc, 2026-10-22: every blocking job passes on Windows, macOS and Linux, including test, conformance and msrv. The non-blocking `linux-adapters` job fails on all four legs; see M5)
- [ ] Add aarch64 runners where available
- [x] Jobs: fmt, clippy, tests, cargo-deny (and SPDX header check)
- [x] Placeholder jobs: conformance, fuzz smoke, docs build, template build, visual artifact upload, benchmarks
- [ ] Linux headless display setup (Xvfb; Wayland headless where possible)
- [x] Cache and MSRV job (Swatinem/rust-cache on every cargo job; the MSRV job also runs the tests under Xvfb; first GitHub run pending)
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
- [x] Support-tag rendering approach decided
- [x] Docs code-sample compile test harness

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
- [x] `WindowBackend` remaining: frameless, always-on-top, transparency, backdrop, drag regions, close handling (Windows: all done; drag regions are Partial, applied to the document loaded at call time; macOS and Linux have their own items)
- [x] `WebviewBackend` trait, M1 subset: load html, navigate (app:// only), eval, post message, set_message_handler (page to shell; the default returns `Unsupported`, so macOS and Linux stay honest until they implement it; Windows and the fake implement it)
- [x] `WebviewBackend` crash callback and reload (verified by a renderer crash through DevTools), intercept (WebResourceRequested), nav policy hooks, doc-start script
- [ ] `WebviewBackend` remaining: snapshot, devtools toggle, profile binding, a11y tree (devtools and context menu are set per window at creation, release off, debug on; no runtime toggle is exposed yet)
- [ ] `Platform` trait (menus, tray, dialogs, clipboard, notifications, deep links, single instance, theme/accent, keystore) (clipboard text and colour scheme are in `tpt-shell-core::platform` with Windows and fake implementations; single instance is in `tpt-shell-platform-windows::single_instance` (Windows only, named mutex and `WM_COPYDATA`, tested in one process and across two processes); the notification-area icon with a context menu is `tpt-shell-platform-windows::tray` and `App::tray` (Windows only; a left click and a menu pick are tested, a right click is not, since it needs a user; the icon is the default application icon; balloon notifications through `Running::notify`); DPAPI keystore, deep-link registration and window-state restore are on Windows too (see their items below); the rest are not started)
- [x] `Clock` and `Rng` traits (system and fake/seeded implementations, unit tested)
- [x] Single UI thread model and `EventLoopProxy` (Send + Sync)
- [x] Minimal built-in async executor and worker pool (no external runtime): `block_on`, `Executor` (`execute`, `spawn`), tested
- [x] Fake backend `tpt-shell-core::fake`
- [x] Unit tests for everything above the platform layer using the fake backend
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
- [ ] First benchmark: binary size, cold start, IPC round trip (binary size is in BENCHMARKS.md; cold start has partial numbers under M1; IPC round trip is not measured)
- [ ] R1/R3 mitigations: strict trait boundary documented; leak test harness for COM objects
- [ ] Done: a Windows hello-world serves embedded assets and round-trips a message. Criterion met on Windows (embedded serving, shim round trip). Other M1 bullets are still open, so the phase stays open until you confirm

---

## Phase M2 — Typed contract, macros, codegen, manifest, capabilities, audit (basic chain)

### tpt-shell-macros
- [ ] `#[command]` with name, capability, kind (sync/async/job/stream), flags (ui, agent) (name, capability and kind = "sync" are done; a command with a capability registers behind its grant; `target(path = "arg")` (or device, url, bytes) names the argument the scope checks, and without it only an unrestricted grant allows the call; `target` without a capability is a compile error; async, job, stream and the ui and agent flags are not)
- [x] `commands![]` (and `register_commands!`, which registers the handlers on a router)
- [ ] `embed_dir!` (final) and `include_manifest!` (`embed_dir!` output reaches the app through `App::assets` and is served from the app origin; the directory is still read at compile time and does not rebuild on new files; `include_manifest!` is not built)
- [x] `Contract` derive macro
- [ ] rust-analyzer friendliness checks
- [x] Compile-fail tests (trybuild-style, licence-checked or hand-rolled) (one bad option was checked by hand; no test suite yet)

### Contract and validation (tpt-shell-ipc)
- [x] Schema model: primitives with bounds, strings with max length/pattern, enums, structs, Option, Vec, string-key maps (all except regex patterns: strings use an allowed-character set instead)
- [x] Scoped types: `ScopedPath`, `DeviceId`
- [x] Validator: unknown fields rejected, bounds checked, scope checked before handler runs (`Router::register_guarded` names a capability and a target; `Router::handle_authorized` checks the window's grant before the handler, and the unauthorized entry points refuse guarded commands. On Windows, page messages reach a handler set with `set_message_handler`, and a runtime test runs a guarded command through `handle_authorized`. App-level wiring exists in the facade (see the App builder item). macOS and Linux do not receive page messages yet)
- [x] `CmdError` with stable code, message, details (`CommandError::with_details` sends `error.details`; invalid args report the offending `path`)
- [ ] Blob channel `app://__tpt/blob/<id>` (Windows serves it: `tpt_shell::blob::put_blob` stores bytes under a random 128-bit id from `BCryptGenRandom` and returns the page URL `https://tpt-app.localhost/__tpt/blob/<id>`, the same DEVIATION as the other app routes; a live test fetches a blob from a page. Still open: macOS and Linux, the 64 MiB store limit is fixed rather than per app, blobs are not removed when a window closes, and the store is per UI thread)
- [x] Stream channel with credit-based backpressure (`tpt-shell-ipc::stream`; not yet carried by a platform transport)
- [ ] Fuzz target: validator (no-panic smoke tests exist; no cargo-fuzz target yet)

### Codegen
- [x] `contract.json` writer (JSON Schema + metadata): `Contract::to_json_string`
- [x] TypeScript client generator (types, JSDoc, runtime bindings): `tpt-shell-ipc::typescript`; wrappers return `Promise<unknown>` because contracts have no result schema yet
- [x] Deterministic output
- [x] Golden tests for contract and TS generation (both have golden tests; the TS output also passes strict `tsc` in a one-off check)
- [x] JS runtime in `js/runtime/` (plain JS, <4 KB gzipped, size check in CI: `tools/check-runtime-size.sh` runs in the `runtime-size` job; currently 945 bytes gzipped, measured locally on 2026-10-22; the job has not run on GitHub yet)
- [x] `tpt.on()` events and `tpt.caps` (`js/runtime/tpt-shell.js` with Node tests in `js/test/`; `Running::emit` sends a Rust event to a window's page, and a live test shows it reaching `tpt.on`; `tpt.caps` reports `webview2` and its version from the user agent. Still open: `emit` is only called from the UI thread, so a background thread needs the event-loop proxy, and `tpt.caps` does not list supported APIs)

### tpt-shell-manifest
- [x] `tpt-shell.toml` parser with line-accurate errors (`TS0001`–`TS0004`)
- [x] Sections: app, windows, capabilities, kiosk, devices, audit, agent, replay, test, profiles, compliance (parse stubs for later phases)
- [x] Published JSON Schema for the manifest (`crates/tpt-shell-manifest/schema/`; a test checks its sections against the parser)
- [x] Golden tests for manifest schema (consistency tests exist, now including app and window keys against the parser; no generated-schema golden yet, and the schema is hand-written)
- [ ] Fuzz target: manifest parser (no-panic smoke tests exist; no cargo-fuzz target yet)

### tpt-shell-capabilities
- [x] Permission engine, default deny (`tpt-shell-capabilities`)
- [x] Per-window and per-origin capabilities
- [x] Scopes: path globs, device identifiers, URL patterns, size caps (path globs: `*` and `?` within a segment, `**` across segments, normalized and validated when granted; path roots, device IDs, URL prefixes and size caps were already done)
- [x] Native consent prompt abstraction (not web content): `ConsentPrompt` trait; the Windows implementation is a message box owned by the requesting window, default No (`tpt_shell_platform_windows::consent`). Its text is not localised yet, and macOS and Linux have no prompt
- [x] Remember-per-profile hook (storage lands in M6): `ConsentStore` trait, in-memory store for now
- [x] Security tests: capability bypass, path-scope escapes (`..`, UNC, device paths, ADS, reserved names, case folding, URL tricks, size caps, and symlink and junction escapes checked on the real path)

### tpt-shell-audit (basic)
- [x] Entry model: seq, wall time, monotonic time, actor, command, capability, parameter hash, optional redacted params, outcome, duration
- [x] Canonical JSON for hashing
- [x] SHA-256 hash chain
- [ ] Redaction policy from manifest (`[audit] store_params` and `redact` parse into `AuditSettings`, validated with the schema; the mapping to `tpt_shell_audit::Policy` is done as `Policy::from(&AuditSettings)`; `App::manifest_audit(manifest, log)` reads the `[audit]` section of the manifest and writes each page command call with its policy; `App::audit(path, policy)` takes a policy directly. Tested. The item stays open because a running app does not yet read the manifest's windows or other sections, and the `[audit]` section is read at build time only)
- [x] Append-only file writer
- [ ] Property tests for chain integrity (seeded randomized and exhaustive single-byte tests exist, for unsigned and signed chains; not a proptest suite)
- [x] Golden tests for audit format

### Security defaults (§9)
- [ ] Only `app://` loaded; remote navigation blocked unless allowlisted per window (Windows enforces `[[windows]] allow_navigation` for top-level and frame navigations, with `https://` prefix matching at a host boundary, tested; the manifest value is parsed but no runtime reads it yet, since the shell does not create windows from the manifest; macOS and Linux not done)
- [x] Strict CSP injected on `app://` responses (Windows: every response from the virtual origin carries `PAGE_CSP`, no inline script or style, verified by a runtime test; the origin is `https://tpt-app.localhost`, see the DEVIATION note above; macOS and Linux not done)
- [ ] Devtools, context menu, view-source disabled in release (devtools and the default context menu are off in release builds on Windows, verified against WebView2 settings; view-source is not checked separately, and macOS and Linux are not done)
- [ ] Release-build CSP override needs an audited manifest flag
- [x] Mock backend generated from contract (basic)
- [x] Tauri-style invoke shim `tpt.compat.tauri` (documented subset)
- [ ] Done: generated TypeScript client calls validated commands; bypass attempts fail

---

## Phase M3 — CLI core, devtools overlay, theme injection, jobs API, error codes

### tpt-shell-cli
- [x] `new` with vanilla and typescript templates (zero Node by default) (both are done; the TypeScript one needs `tsc` to build, which is the one place Node is used)
- [ ] `dev`: file watcher, hot reload preserving Rust state, mock toggle (`tpt-shell dev <dir>` reports added, changed and removed files every half second; no hot reload into a running window yet, and no mock toggle)
- [x] `dev`: automatic replay of last session after reload (hook; full replay in M8)
- [x] Dev server (hand-rolled HTTP or licence-clean crate) (`tpt-shell dev` serves GET and HEAD on 127.0.0.1 only, refuses paths that leave the directory, and has tests with a real socket; the server answers one request at a time)
- [x] `build` (release build; compliance outputs added in M10)
- [x] `doctor` (WebView2 version, toolchain, licence policy): the licence policy checks the deny.toml allow list against cargo metadata, read-only
- [ ] `contract gen | check | diff` (breaking-change detection: `tpt-shell contract diff <old> <new>` and `contract check <new> --baseline <old>` are done, in `tpt-shell-ipc::contract_diff`; they flag removed commands and fields, new required fields, capability changes, tighter limits, removed enum values and type changes. `gen` is not built: the contract is made inside the app by `commands![]`, and no app entry point writes it out yet)
- [x] `manifest check | explain` (`manifest check` done; `explain` is `tpt-shell explain`, see below)
- [x] `explain <code>`
- [x] Error code registry `TS####` with explanations and fix hints (`tpt-shell-errors`)
- [x] Errors point at manifest lines (`tpt-shell manifest check` prints the line and the fix hint)
- [x] `cargo install tpt-shell-cli` works from a local path (checked with `--root` into the scratchpad)

### Jobs API
- [x] `#[command(job)]` spawn, progress, cancel (job runtime and attribute built; pages cannot call `job.cancel` yet, see the open item below)
- [x] Let pages cancel their own jobs: `job.cancel` is guarded (capability `job.cancel`) and cancels only jobs the calling window started
- [ ] Streamed results with backpressure (job results are one value; streams are a separate channel in `tpt-shell-ipc::stream`)
- [x] Typed job events in TS client (`job.cancel()`)
- [x] Optional progress component
- [ ] Tests with fake backend (jobs are tested directly on threads, not through the fake backend)

### Theming
- [x] Inject `--tpt-accent`, `--tpt-bg`, `--tpt-fg`, `--tpt-font` etc. (`tpt-shell-core::theme`, applied at document start on Windows; a change applies to windows created after `set_theme`)
- [x] Keep `prefers-color-scheme` in sync (the WebView2 preferred colour scheme follows the system setting; the CSS result is not checked on screen)
- [x] Signal high contrast, reduced motion, reduced transparency (Windows reads `SPI_GETHIGHCONTRAST`, `SPI_GETCLIENTAREAANIMATION` and the `EnableTransparency` value when a window is created, and the page gets the attributes; a live test checks them. Open pages follow later changes, see the colour scheme item)
- [x] Windows Mica/Acrylic/Tabbed backdrop and dark title bar
- [x] Normalisation shim (opt-in): viewport, scrollbar, font stack, drag region helper, safe-area insets

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
- [x] Dialogs (open, save, message, folder) via IFileDialog (all four built; tests check the mapping and SDK slots, not the dialogs on screen)
- [x] App builder in the facade (`tpt_shell::App`): windows, grants and commands; each window's page messages go through the capability-checked router with that window's grants; a grant that needs consent asks in a native dialog owned by the window (default No); runtime tests run an app end to end, with the dialog answered Yes and No. Windows only, with `App::loader` set explicitly. Pages load from the app origin through `App::asset` (the old `Window::html` placed pages at a data: origin and is gone). Commands come from `#[command]` through `App::register`, and assets from `embed_dir!` through `App::assets`. Still missing: async handlers, jobs and streams, the worker pool, and `include_manifest!`
- [x] Clipboard (text first): `tpt-shell-platform-windows::clipboard`, restores the user's clipboard in its test
- [ ] Window state restore, fullscreen, multi-monitor (`App::remember_window_state` saves each window's position, size and maximized state to a JSON file when the event loop ends and restores it at the next start; live test. Fullscreen existed already. Multi-monitor placement is not done)

### Early measurements
- [ ] Time the three-command quickstart on a clean Windows machine
- [ ] Baseline benchmarks committed
- [x] Decide (O3): vanilla and TypeScript only for now. The other templates listed in M11 are added there
- [ ] Done: the three-command quickstart works on Windows in under five minutes

---

## Phase M4 — macOS backend and conformance parity with Windows

### Conformance suite foundation
- [x] `conformance/` harness and fixture pages
- [ ] Checks: IPC round trip, large blobs, streams/backpressure, custom protocol, CSP, navigation policy, theme injection, snapshot, drag regions, DPI
- [ ] Run on Windows first, establish baseline
- [ ] Compatibility report generator (published artifact)

### macOS backend (tpt-shell-platform-macos)
- [x] Decide: objc2 (named in spec §6.2 as an allowed low-level FFI crate). Its licence is checked with cargo-deny when M4 adds it
- [ ] Own ObjC class/delegate declaration macro (R3)
- [ ] NSApplication run loop and EventLoopProxy
- [ ] NSWindow: bounds, title, fullscreen, frameless, always-on-top, transparency (all written against the generated bindings and type-check for macOS with clippy; none has run on a Mac yet. Tick after the macOS CI run)
- [ ] NSVisualEffectView vibrancy and appearance observation (`set_backdrop` and `theme::observe_appearance` are written and type-check; unrun. Mica, Acrylic and Tabbed map to approximate system materials, not the Windows effects)
- [ ] WKWebView with WKUserContentController script message handler (written in `webview.rs`; unrun)
- [ ] WKUserScript at document start (written; unrun)
- [ ] WKURLSchemeHandler for `app://` (written; unrun)
- [ ] WKNavigationDelegate (navigation policy, crash callback) (navigation policy is written; the crash callback, `webViewWebContentProcessDidTerminate`, is not)
- [ ] `takeSnapshot` (written, PNG through `NSBitmapImageRep`; unrun)
- [ ] NSOpenPanel / NSSavePanel (written, plus an NSAlert message box; type-checked only, the modal dialogs have no runtime test. The message box is application-modal and ignores the owner window; filters use the deprecated `allowedFileTypes`)
- [ ] NSPasteboard (text, HTML, image, files) (text is written; HTML, image and files are not built)
- [ ] UNUserNotificationCenter
- [ ] NSStatusItem tray and native menus (the tray with a text menu is written; unrun. Native application menus are not built)
- [ ] Deep links via Apple Events
- [ ] Single instance via NSRunningApplication with arg forwarding (written, forwards through a distributed notification, no sockets; needs a bundle id, so a bare binary gets `Unsupported`; two launches in the same instant can both become primary; unrun)
- [ ] Keystore (Keychain) (Security framework generic-password items, `WhenUnlockedThisDeviceOnly`; written, unrun)
- [ ] NSAccessibility tree access
- [ ] Global hotkeys (Partial)
- [ ] Support tags: macOS 12+ matrix, profile limits on 12–13

### Parity
- [x] Conformance green on macOS CI (run 11: the job passes; pending and skipped checks are listed in the compatibility report, so green means no failures, not full coverage)
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
- [ ] GTK4 + WebKitGTK 6.0 adapter (`gtk4/` is written and type-checks for x86_64 and aarch64 Linux; no GTK call has run. It runs only in the non-blocking CI job `linux-adapters`; tick when those legs are green. The first symbols to check if a leg fails: `webkit_web_view_get_context` and the `gdk_clipboard_store_async` arguments)
- [x] Runtime adapter selection and `doctor` check (GTK 4 preferred when both load; `TPT_GTK=3|4` forces one with no fallback and a typo is an error; with neither installed the error names both toolkits and the apt packages; `doctor` prints the report on Linux. The selection logic is tested on the host; the doctor output has not been seen on Linux)
- [ ] GTK main loop and EventLoopProxy (GTK4 is a `GMainLoop` with a `g_idle_add` proxy; type-checked, tested in CI only)
- [ ] Window features: bounds, title, fullscreen, frameless, always-on-top, transparency (GTK4 has no bounds or always-on-top API, so they return `Unsupported` by design; transparency needs a compositor)
- [ ] WebKitUserContentManager messages and scripts (GTK4 written, CI-only test)
- [ ] `webkit_web_context_register_uri_scheme` for `app://` (GTK4 written, CI-only test)
- [ ] Navigation policy and crash signals (GTK4 written: `web-process-terminated` reloads up to five times; CI-only test)
- [ ] `webkit_web_view_get_snapshot` (GTK4 encodes a `GdkTexture` to PNG; CI-only test)
- [ ] GtkFileChooser dialogs (GTK4 uses `GtkFileDialog`, 4.10+; on 4.6 only file dialogs return `Unsupported`; only the validation is tested)
- [ ] Clipboard (GtkClipboard / GdkClipboard) (GdkClipboard is written; the X11 round trip is tested in CI only; Wayland read-back is not asserted)
- [ ] Tray (StatusNotifier over D-Bus; GtkStatusIcon fallback)
- [ ] Freedesktop notifications over D-Bus
- [ ] Single instance over UNIX socket
- [ ] Deep link handling (desktop file registration) (Windows: `App::deep_link(scheme)` registers the scheme under HKCU and a later link reaches the running app through single instance; registration is tested. Linux desktop file is not written)
- [ ] Keystore (Secret Service)
- [ ] AT-SPI accessibility dump
- [ ] Global hotkeys X11-only (Partial tag)
- [ ] Wayland and X11 verification (feature differences are written up in `docs/src/platform-notes.md`; nothing is verified yet)
- [ ] D-Bus client written without a non-compliant dependency, or licence-checked crate

### Parity
- [ ] Conformance green on Ubuntu CI (Xvfb, Wayland headless where possible)
- [ ] Both GTK3/4 adapters covered in CI matrix (R4) (the `linux-adapters` job has four legs, gtk3 and gtk4 each under Xvfb and headless Weston, with `continue-on-error`; tick when green and remove that line)
- [ ] aarch64 Linux build verified (the crate type-checks for `aarch64-unknown-linux-gnu`; no build or run)
- [ ] Compatibility report covers all three OSes
- [x] Decide: real Linux hardware needs (O6), per the O6 decision above
- [ ] Done: Linux passes the conformance suite with parity (both adapters)

---

## Phase M6 — Profiles and ephemeral sessions (§10.7); kiosk runtime and supervisor (§10.1)

### tpt-shell-profiles
- [x] Profile kinds: default, named, ephemeral (`tpt-shell-profiles`)
- [ ] Window-to-profile binding (`App::profile(ProfileKind::Named | Ephemeral)` chooses the folder the windows keep their data in; every window of an app shares that folder, so per-window profiles are not built)
- [ ] Windows mapping: WebView2 profile name / user data folder
- [ ] macOS mapping: data-store identifier (14+), non-persistent otherwise
- [ ] Linux mapping: WebsiteDataManager base dirs / ephemeral
- [ ] `tpt.caps` reports profile capability level
- [ ] Wipe on close, wipe on idle (an ephemeral session is wiped when the app stops, through its guard; the idle wipe and wipe on window close are not wired, because WebView2 holds the files while a window is open)
- [x] Secure erase (best-effort) with documented SSD/journaling limits (`wipe_dir` overwrites file contents before removal; the limits are in its doc comment)
- [x] Quotas (`usage` and `enforce_quota`; not yet applied to a live WebView2 profile)
- [x] Profile manager API: create, list, switch, export (encrypted), delete (encrypted export: ChaCha20-Poly1305, key from PBKDF2-HMAC-SHA256 at 600,000 iterations; switch takes effect at the next start)
- [x] Per-profile storage of consent decisions (`FileConsent` in `tpt-shell-capabilities` keeps answers in `consent.txt` in the window's data folder, so a named profile keeps them and an ephemeral session drops them with its folder; an unreadable file falls back to memory)
- [ ] Conformance: isolation (A writes invisible in B) and no-residue for ephemeral (file-level tests pass; no WebView2 storage conformance test yet)
- [ ] Done: isolation and no-residue tests pass on all three OSes

### tpt-shell-kiosk
- [x] `[kiosk] mode = "single-app"` full-screen, single window, allowlisted navigation
- [ ] Disable devtools, context menu, zoom, optional text selection, drag-drop, print shortcuts, function keys, swipe gestures
- [x] Windows low-level keyboard hook (Alt+Tab, Win, Alt+F4, Ctrl+Esc), best effort
- [ ] Document OS lock-down recipes: Windows Assigned Access, macOS guided access/MDM, Linux dedicated session (R10)
- [ ] `app.snapshot_state()` on interval and on exit, typed state store, restore on restart (`App::snapshot_state(path, every, capture, restore)` saves what `capture` returns on a UI-thread timer and when the event loop ends, and calls `restore` once with the saved value before the windows show; live test. Typed state is `serde_json::Value` for now, not a derived struct; a corrupt file is reported and skipped)
- [ ] Page crash recovery: reload to last good state
- [ ] Idle reset: home route, clear session, ephemeral integration (`App::kiosk_config` with `idle_seconds` resets every window to the `home` page after that long with no page messages; live test. Clearing the session and the ephemeral profile are not done)
- [ ] Multi-monitor placement, rotation, per-monitor kiosk windows
- [ ] Signed local config (Ed25519) (`App::kiosk_config` loads the config only when its signature checks out, and rejects a `home` path that leaves the app; tested. The signing tool is not written, and the config does not yet set the windows or the policy beyond home and idle)
- [x] Local admin endpoint (off by default, token protected)
- [x] Heartbeat file and optional local HTTP health endpoint (off by default)

### tpt-shell-supervisor (separate binary)
- [ ] Spawn app and watch heartbeat over local IPC (the app is spawned with `TPT_HEARTBEAT_FILE` and beats by rewriting that file, which the supervisor reads by age; this is local file IPC, not a pipe or socket, since the shell opens no sockets. A named-pipe transport is still open)
- [x] Restart on crash or hang (`ProcessSpawner` runs the app, stops it after a missed heartbeat or at the scheduled time, and `tpt-shell-supervisor` restarts it with backoff; tested with real child processes)
- [x] Exponential backoff with crash-loop cap (`Backoff`, `CrashLoopGuard`)
- [x] Alert hook (command or file) (`--alert-command` and `--alert-file`; tested)
- [x] Scheduled restart (e.g. 03:00) (`Scheduled::is_due`, across midnight too; `ProcessSpawner` stops the run at the time and the loop restarts it without counting a crash; `--restart-at HH:MM` uses local time on Windows and UTC elsewhere)
- [x] Complete supervisor logs (`--log` appends a timestamped line for each run's end, restart, and give-up, and rotates to `.1` at 1 MiB)
- [ ] Idle RAM ≤ 5 MB measured (measured 2026-10-22 on Windows: private memory 3.2 MB, working set 7.0 MB while supervising a healthy child. The budget is met on private memory; the working set is larger because it counts shared system DLL pages. Which measure §13 means is still open)

### Boot integration and CLI
- [ ] `tpt-shell install-service`: Task Scheduler/Startup entry, LaunchAgent, systemd user unit (Windows: `tpt-shell install-service <name> <program>` and `uninstall-service` write a per-user `Run` entry; tested. Task Scheduler, LaunchAgent and systemd are not done)
- [ ] `tpt-shell add kiosk`
- [ ] `kiosk` template (preview, finalised in M11)

### Soak
- [x] Fault-injection harness: crashes, hangs, page failures
- [ ] 72-hour soak on each OS with memory growth checks
- [ ] Done: 72 h soak passes; app returns to configured state each time; supervisor logs complete

---

## Phase M7 — Device bridge, virtual devices, printers and scanners (§10.2)

### tpt-shell-devices core
- [x] `Transport` trait (`tpt-shell-devices::transport`: write, read, and an error type with `Busy` for backpressure; control transfers are not in the trait yet)
- [x] Device ID and scope types (vendor/product, port globs, host:port): `tpt-shell-devices::id`
- [ ] API: `devices.list(filter)`, `open(id, options)`, read/write streams with backpressure, control transfers (`DeviceHub::list` and `open` check the scope and each device opens once; `open_with(id, scope, &OpenOptions)` and `discover(scope)` are built, and `OpenDevice::control` is audited (`EventKind::Control`); control transfers work on Linux usbfs only; backpressure is `DeviceError::Busy`, not a stream, so the line stays open)
- [ ] Discovery and opt-in hotplug events (polling `Watch::poll_native` is built for serial, hidraw, usbfs and IOKit HID on macOS and Linux, and does nothing when disabled; Windows lists nothing and there is no OS event stream: udev netlink, IOKit notifications and `WM_DEVICECHANGE` are not built)

### Transports
- [x] Serial: Win32 COM
- [ ] Serial: termios (macOS, Linux) (`platform/unix_serial.rs`, non-blocking, `flock`, refuses a node that is not a terminal; type-checked for both targets, never run. Its runtime test needs `TPT_TEST_SERIAL_PORT`, so CI only runs the device-free checks. Tick after a run with a real or pty-backed port)
- [ ] HID: Windows HID (not built; returns `NotSupported`)
- [ ] HID: IOKit (hand-written FFI for `IOHIDManager`, type-checked only, never run on a Mac; the riskiest unrun code)
- [ ] HID: hidraw (Linux, udev notes) (written with discovery from `/sys/class/hidraw`; the udev rules note is in `docs/src/guides/devices.md`; the runtime test skips without a device or permission, so unrun)
- [ ] USB raw: WinUSB (not built; returns `NotSupported`)
- [ ] USB raw: IOKit (not built; returns `NotSupported`)
- [ ] USB raw: usbfs (bulk and control written, type-checked; skips without a device; refuses bulk when another driver owns the interface; not built on MIPS, PowerPC or SPARC)
- [x] TCP/UDP raw sockets (network printers), behind feature `net-devices`, off by default

### Helpers
- [x] ESC/POS builder (`tpt-shell-devices::escpos`, golden bytes)
- [x] ZPL builder (`tpt-shell-devices::zpl`, golden output and escaping)
- [x] Receipt/label printing
- [x] Raw print via OS spooler (Windows Full through winspool; macOS and Linux Partial through `lp`)
- [ ] Barcode scanners: keyboard-wedge, serial, HID POS
- [ ] Scales: common serial protocols
- [ ] Customer displays

### Security and audit
- [ ] Capabilities `device.serial`, `device.hid`, `device.usb`, `device.net`, `device.print` with scopes (a page command under `device.print` with a `Devices` scope opens a device through the hub and prints; live test with a virtual printer. The other kinds and the consent prompt for devices are not wired)
- [ ] Native consent dialog on first use, remembered per profile
- [x] Audit every open, close, write (`DeviceHub` reports each to its audit sink with the byte count; reads are not audited)
- [x] Payload logging off by default; hash-only or bytes per manifest (only byte counts are logged; the option to log bytes or hashes per manifest is not built)
- [ ] Security tests: scope escapes, bad device IDs (a scope cannot open another kind of device or a device outside its glob; bad IDs are refused by the parser. Symlink and device-path escapes do not apply to the virtual devices yet)

### Virtual devices
- [x] VirtualSerial (a loopback port with a capacity limit)
- [x] VirtualHid (input and output reports, through a handle)
- [x] VirtualPrinter (ESC/POS receipt preview: bold, alignment, feeds and cuts)
- [x] VirtualScanner (scripted scans, sent as the text and a carriage return)
- [ ] Selection by manifest or env var (`virtual_devices::from_spec` parses `kind@id;...`, meant for `TPT_VIRTUAL_DEVICES`; the environment variable is not read by the app and the manifest does not name devices yet)

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
- [x] Cargo feature `test-driver`, per-run token, local IPC (file-based channel, no socket or pipe, per the no-sockets rule)
- [ ] Rust API: `find(role().name())`, click, type_text, press, screenshot, dom_snapshot, a11y_tree, eval (test builds only), wait_for
- [x] Expectations with automatic retry
- [ ] Screenshots: CapturePreview, takeSnapshot, get_snapshot
- [ ] `tpt-shell test` with artifact collection
- [ ] Integration with virtual devices and replay
- [ ] Fuzz and security tests on control channel (R7)

### Visual diff
- [x] Own perceptual comparer with region masks and per-region tolerance (`tpt-shell-test::visual`: raw RGBA input, YIQ-weighted distance, ignore and tolerance regions; PNG decoding and the CLI are not built)
- [ ] `tpt-shell visual compare` merging artifacts from three CI jobs
- [x] HTML report
- [ ] Threshold failures (the comparer reports pass or fail against `max_changed_fraction`; no CLI or CI job fails on it yet)
- [ ] WebView2 channels (stable/beta/canary) support on Windows
- [x] Decide: visual baseline store implementation (O5): Git LFS in the repo

### Accessibility audit
- [x] Rule engine for WCAG 2.2 AA subset: names/roles, contrast, focus order, keyboard traps, target size, form labels, reduced motion (names read from the live tree need the platform dumps)
- [ ] Native tree dump: UIA
- [ ] Native tree dump: NSAccessibility
- [ ] Native tree dump: AT-SPI
- [x] DOM-vs-native comparison report
- [x] Seeded-violation fixture page
- [ ] Devtools a11y quick check

### Record/replay (tpt-shell-replay)
- [ ] Recorder: IPC calls with args/results/timing, DOM input events with stable paths, window events, protocol loads
- [x] Virtualised Rust `Clock`/`Rng` in record/replay mode
- [x] In-page shim for Date.now, performance.now, Math.random, timers (record/replay only)
- [ ] `.tptreplay` format: zstd JSON Lines + assets + version (JSON Lines and version 1 are done; compression and assets are not)
- [x] `docs/replay-format.md` (format, readers, divergence and limits)
- [x] Golden tests for replay format
- [ ] Fuzz target: replay reader
- [ ] Player with strict and lenient modes, hidden window for CI (the strict and lenient readers exist; there is no player window)
- [x] Divergence reports (`tpt-shell-replay::divergence`, first difference by kind and data)
- [ ] `tpt-shell replay`
- [x] `tpt-shell report` bug-report bundle with redaction hooks
- [x] Documented determinism limits (R8) (limits are in `docs/replay-format.md`; the R8 review is not done)

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
- [x] Ed25519 checkpoints every N entries or T seconds (both intervals in `tpt-shell-audit`; the time check runs when an entry is written, so a quiet log checkpoints with its next entry. The key still comes from the caller; the keystore item below covers that)
- [ ] Keystore: DPAPI, Keychain, Secret Service (DPAPI on Windows: `tpt-shell-platform-windows::keystore` encrypts each secret for the current user and stores one file per name; tested. Keychain and Secret Service are not done, and the audit checkpoint key does not use the keystore yet)
- [x] HSM hook for key supply
- [ ] External anchor hook: file on other volume, syslog, HTTP
- [x] Rotation and size limits
- [x] Export: JSON Lines, CSV (`tpt-shell audit export --format csv|jsonl`; both verify the chain first, and the CSV leaves out parameters and escapes formula-like cells)
- [x] `tpt-shell audit verify | export | view` (`verify --key <public-key-file>` checks checkpoint signatures; without a key it says they were not checked)
- [x] Verify reports first bad sequence number (`verify_file` and `verify_file_with_key`)
- [ ] Devtools audit viewer panel
- [x] Tamper tests: flip, drop, swap, truncate all detected (flip, drop, swap, re-chain and forged signatures are tested; truncating the tail is a documented limit that needs an external anchor)
- [x] Property tests for the full chain with checkpoints (a seeded suite of 30 random signed chains, with byte flips, removed and swapped records, each caught; tail truncation is left out, as the anchor item covers it)
- [ ] Document limits (compromised host, role of anchoring)
- [ ] `audit-app` template and `audit-viewer` example (preview)

### Agent (tpt-shell-agent)
- [ ] Cargo feature gate; off unless build and manifest enable an agent profile
- [x] Semantic tree from DOM + ARIA: roles, names, states, values, bounds, snapshot-scoped refs (`js/agent/agent.js` and `tpt-shell-agent::page`; a live test snapshots a real page)
- [ ] Action API: click, type, select, scroll, focus, key, wait_for, read, invoke (click, focus, type, select, key, scroll and read work on a real page, and a stale reference is refused. `wait_for` and `invoke` are not built. Key events are synthetic, so they do not trigger the browser's own key handling)
- [x] No raw JavaScript evaluation (the agent crate has no evaluation path, and its page scripts are fixed: `tpt-shell-agent::page` builds only the snapshot and action calls, never caller-supplied code; tools are the only callable surface)
- [ ] `[agent]` profiles: actions, routes, rate limits, confirm-required commands, max session length (rate limits, confirm-required commands and max session length are in `tpt-shell-agent::Policy`; there are no manifest profiles or route rules yet)
- [x] Redaction: password fields and `data-tpt-redact` (a password value and a redacted block never reach the snapshot or a read; live test)
- [x] Native confirmation dialogs for sensitive actions (Windows message box, default No, owned by the window when given; the facade wiring is open)
- [x] Kill switch (hotkey and API) (Partial: Windows hotkey Ctrl+Alt+Shift+K through `KillHotkey`, which fails if another app holds the combination; the facade wiring is open)
- [x] MCP server over stdio (`tpt-shell-agent::serve`: initialize, tools/list, tools/call; no resources yet)
- [ ] MCP server over localhost HTTP with token
- [ ] Tools generated from agent-flagged commands plus UI tools (tools are passed in explicitly; there is no agent flag on commands and no UI tools yet; the page actions exist as scripts but are not offered as tools, because the session would have to run on the UI thread with the window)
- [ ] Resources: semantic tree and audit tail (the snapshot exists; it is not yet an MCP resource, and the audit tail is not exposed)
- [ ] Prompt-injection hygiene: untrusted markers, allowlist navigation (page text in a snapshot is wrapped in `<untrusted>` markers by `render`; that is a convention the model must follow, not a guarantee. Tool results are not yet marked, and navigation is not restricted for agents)
- [ ] All agent actions audited and recordable to replay (every call is logged in the session; nothing writes it to the audit file or replay yet)
- [ ] Policy tests: forbidden actions, rate limits, redaction, kill switch (forbidden actions, rate limits, confirmation, kill switch and session length are tested; redaction is tested on a real page through `tpt-shell-agent::page` and the live agent test, not yet through the policy layer)
- [ ] Fuzz and security tests on control channel (R7)
- [ ] `tpt-shell add agent`, `agent-demo` template and example
- [ ] Done: example agent completes a scripted task; policy tests pass; every action appears in the audit log

---

## Phase M10 — Compliance outputs, pinned runtime, vendoring, reproducible builds (§10.8); bundling and signing

### tpt-shell-compliance
- [x] CycloneDX 1.5 SBOM (`sbom.cdx.json`): `tpt-shell-compliance`, written by `tpt-shell compliance`; valid against the published 1.5 schema (`tools/validate-sbom.sh`)
- [x] SPDX 2.3 SBOM (`sbom.spdx.json`): same command; valid against the published 2.3 schema (`tools/validate-sbom.sh`)
- [ ] `licenses.html` and embedding at `app://__tpt/licenses`
- [x] `THIRD_PARTY_NOTICES` generation
- [x] `attestation.json` (rustc, lockfile hash, output digests, policy result, git commit, build flags, reproducibility settings incl. SOURCE_DATE_EPOCH)
- [x] Optional detached Ed25519 signatures
- [x] Policy gate using the same cargo-deny logic as CI (the gate follows the `deny.toml` allow list with its own SPDX parser; it is not cargo-deny itself)
- [x] SBOM schema validation tests (CycloneDX and SPDX) (`tools/validate-sbom.sh` runs both against the published schemas; not yet a CI job)
- [x] Deliberately-bad-dependency test fails the build (the gate exits 1 on a forbidden licence and is unit-tested; nothing runs it from the build yet)

### Runtime and air-gap
- [ ] `tpt-shell runtime pin | list` (WebView2 Fixed Version fetch + bundle)
- [ ] Shipped app never contacts Microsoft
- [ ] macOS/Linux minimum versions recorded; `doctor` checks them; limit documented
- [x] Test: shell opens no sockets by default
- [x] `tpt-shell vendor`
- [ ] `tpt-shell docs --offline`
- [x] Reproducible-build guidance and flags (`--locked`, path remapping, `SOURCE_DATE_EPOCH`)
- [ ] Reproducibility test: two builds of same commit produce identical artifacts, or diff explained
- [x] Procurement pack template (questionnaire, data-flow diagram, SBOM, licence summary, audit/kiosk explainer)

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
- [ ] Five-minute quickstart (`docs/src/quickstart.md` covers the hello window, scaffolding, the manifest check and the dev preview, each run here on Windows; a scaffolded project still does not open a window by itself, and the timed run on a clean machine is not done)
- [x] Concepts
- [x] Cookbook with 20+ recipes
- [x] Guides: kiosk deployment, devices, audit, agents, profiles, compliance
- [x] Security guide
- [x] Platform notes and troubleshooting
- [ ] API reference with rendered support tags
- [x] Migration guides: Tauri, Electron
- [ ] Examples gallery
- [ ] Published compatibility report
- [ ] Docs code samples compiled in CI
- [x] Offline docs bundle verified
- [ ] Publish docs site (per O5)

### Editor and CI tooling
- [x] VS Code extension: manifest schema and completion
- [x] VS Code extension: contract explorer
- [x] VS Code extension: run/debug configs (tasks and launch snippets; the debug type runs the task in a terminal, since the app has no debug protocol)
- [x] VS Code extension: audit viewer
- [x] VS Code extension: inline error codes
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

## Phase M11b — Adoption tooling and stub clean-up (added after the 2026-10-22 project review)

The review found no `todo!()`, `TODO` or `FIXME` markers; unfinished work shows up as `Unsupported` returns and the unticked items above. These items close the gap between "it exists" and "someone can pick it up and use it".

### Finish in-flight work
- [x] Linux: declare `mod webkit_context` in `crates/tpt-shell-platform-linux/src/lib.rs` (the file is untracked and not compiled, so its tests do not run)
- [x] Linux: replace the `TPT_WEBKIT_CONTEXT == "own"` checks in `gtk3/mod.rs` and `gtk4/mod.rs` with `ContextConfig::from_env`, and settle the default (the module says owned is the default; commit 388ba9b made it opt-in)
- [x] Linux: persistent storage for the owned context (GTK3 `WebsiteDataManager`, GTK4 `webkit_network_session_new(data, cache)`, symbols in `sys.rs`) and `with_data_dir` on both backends; verified only by the `linux-adapters` CI job
- [x] README: replace the stale "M0 in progress, no usable API yet" status with the real state, a quickstart and an example snippet
- [x] Docs that promise more than exists: `quickstart.md` "Not yet", `cookbook.md` replays, `security.md` Secret Service / fuzz targets / compliance outputs

### Onboarding
- [x] `tpt-shell new` writes a runnable project (`Cargo.toml`, `src/main.rs`, manifest, assets) so `cargo run` opens a window (`new rust`: builds here, and a run stops with a clear message until `WebView2Loader.dll` is beside the program; a window has not been seen open)
- [x] `tpt-shell new --template <name>` for the Rust templates (`pos`, `kiosk`, `audit-app`, `agent-demo`, `appfront-dom`), not only `vanilla` and `typescript` (`pos` built here; the others copy the same way but are not built)
- [x] `tpt-shell init` adds tpt-shell to an existing crate
- [x] `tpt-shell run`: build and launch in one command
- [ ] WebView2Loader.dll: locate or embed it so `App::loader(path)` is optional and the quickstart has no manual download step (done: `App::loader` is optional now, and the app finds `WebView2Loader.dll` beside the program. Not done: embedding it, which needs a decision on redistributing Microsoft's file, and a clean-machine run)
- [ ] `tpt-shell dev`: reload the page in the window on change (note: the premise was out of date. Served pages already reload in a browser tab on change. The shell window loads `app://` assets, not the dev server, so it does not reload; that needs the window to load the dev server URL in dev mode)
- [x] `tpt-shell dev`: a mock `window.tpt` so a page works in a plain browser tab (`js/runtime/tpt-shell-mock.js`, served and injected by `dev`; checked in Node and against the live server)
- [ ] `tpt-shell doctor`: check the loader DLL, the project manifest and the Linux GTK/WebKit libraries; print a fix hint per failure from `tpt-shell-errors`; optional `--fix` (done: loader DLL location (warning), project manifest with its registry hint, Linux libraries. Not done: `--fix`; the WebView2 runtime and licence failures print a hint inline, not from the registry)
- [x] `tpt-shell contract gen`: write the contract and the TypeScript client from an app entry point (two steps: the app runs with `--tpt-write-contract <file>`, which already existed, then `tpt-shell contract gen <file> <dir>` writes `commands.ts`. Checked end to end with a scratch app holding one command)
- [ ] Time the three-command quickstart on a clean machine and record the result

### Distribution
- [ ] Release workflow (`.github/workflows/release.yml`) using the existing cargo-dist config
- [ ] Publish-ready crate metadata (`publish = false` and version `0.0.0` today)
- [ ] npm package for `js/runtime` and the generated TypeScript client
- [ ] Deploy the mdBook docs to GitHub Pages
- [ ] Publish the VS Code extension

### Ideas to evaluate
- [x] `tpt-shell permissions suggest`: propose the smallest capability grants for an app from its commands and pages (commands only, from the contract file. Pages are not analysed, and the output says so)
- [ ] Capability dry-run in `dev`: log would-be denials instead of failing, with a copy-paste manifest fix (not built as a dry-run: letting a denied call through, even in dev, weakens the capability check. The copy-paste fix is covered by `tpt-shell permissions suggest --audit <log>`, which reads the `CAP_NO_GRANT` refusals from a real run. Decide whether a dev-only bypass is wanted before building it)
- [x] `tpt-shell audit explain`: plain-language summary of an audit log for non-technical reviewers (verifies the log first; the summary is unit-tested on synthetic records, not on a log written by an app)
- [x] `tpt-shell kiosk init`: signed kiosk config plus service install in one command (`kiosk init <dir> --key <file>` writes and signs the starter config; the sign-in install runs only with `--install <program>`. It needs a key you already have, and it never overwrites a config. The install path is the existing `install-service`, not run here)
- [x] `tpt-shell report` output re-openable with `replay` (`replay` opens a bundle's replay. A bundle has the replay text but not the asset bytes, only their names and sizes, so an app with assets cannot replay all of it. A bundle made without `--replay` says so)

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
- [x] Hello-world binary ≤ 4 MB stripped on Windows (`examples/hello`: 706,048 bytes release on 2026-10-22; see BENCHMARKS.md)
- [ ] Cold start to first paint ≤ 500 ms; shell overhead ≤ 50 ms
- [ ] IPC round trip small payload p95 ≤ 5 ms
- [ ] Blob channel ≥ 50 MB/s sustained
- [ ] Idle shell memory overhead ≤ 25 MB
- [ ] Supervisor ≤ 5 MB idle (see the M6 item: private memory 3.2 MB, working set 7.0 MB)
- [ ] `BENCHMARKS.md` with methodology and results (created with the method and the binary-size result; cold start, IPC, blob, memory and the macOS and Linux numbers are still to add)
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
