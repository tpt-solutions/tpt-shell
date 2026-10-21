# RFC 0001: WebView2 COM layer (Windows backend)

Status: **Accepted with recommended defaults** (you asked to follow recommendations). Implemented for environment, controller and webview. Refcount and leak tests remain.
Spec: §6.1 (Windows backend), §9 (unsafe confined), R3 (COM leaks and crashes), O1 (loader, decided as OfficialLoader).
Author: TPT Solutions. Date: 2026-10-20.

## Summary

Create a WebView2 environment and controller from Rust using hand-written COM vtables for the subset we use. Load `WebView2Loader.dll` through the existing `OfficialLoader` (`crates/tpt-shell-platform-windows/src/loader.rs`).

## Motivation

M1 needs a window that hosts a webview, loads `app://` content and round-trips a message. That needs the environment and controller calls. Everything above them depends on this layer, so its design affects R1 and R3.

## Design

### Entry points

Resolved by `OfficialLoader` (checked against the NuGet 1.0.4312-prerelease header, `WebView2.h`):

- `CreateCoreWebView2EnvironmentWithOptions(PCWSTR browserExecutableFolder, PCWSTR userDataFolder, ICoreWebView2EnvironmentOptions*, ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler*)`
- `ICoreWebView2Environment::CreateCoreWebView2Controller(HWND, ICoreWebView2CreateCoreWebView2ControllerCompletedHandler*)`
- `ICoreWebView2Controller::get_CoreWebView2(ICoreWebView2**)`

Interface IIDs come from the same header, for example `IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler = {4e8a3389-c9d8-4bd2-b6b5-124fee6cc14d}`.

### Vtables

- Each interface we use is a `#[repr(C)]` struct holding a pointer to a `#[repr(C)]` vtable. The vtable begins with `IUnknown` (QueryInterface, AddRef, Release) and then lists methods in the exact order of the header.
- Only methods we call are declared, but the slots before them must still be declared as placeholders. Unused slots are typed as `unused_slot: usize`.
- Each slot we call is checked by a test that reads `WebView2.h` from the package and asserts the method at that header index has the name we expect (implemented as `used_slots_match_webview2_header`). Unused slots are placeholders sized from the header, so each called slot sits at its header index. This catches reordering, which would otherwise cause a crash at runtime. The test runs in CI only when the header is available, and is skipped with a message otherwise.

### Callback handlers

- Handlers we implement (environment-created, controller-created, web-message-received, and so on) are C++-style objects in Rust: a `#[repr(C)]` struct whose first field is a pointer to a static vtable, followed by a reference count and a boxed Rust closure.
- A small internal macro (`com_object!`) generates the vtable, QueryInterface for the handler IIDs and `IUnknown`, and the Release that frees the box when the count reaches zero (R3).
- Handlers are created on the UI thread. Callbacks from WebView2 arrive on the UI thread, so the closure runs there. Work from other threads goes through `EventLoopProxy` (spec §5.3).

### Threading

- COM is initialised once per UI thread with `CoInitializeEx(COINIT_APARTMENTTHREADED)`. The `UI thread` is the only thread that touches windows and webviews (spec §5.3).
- The environment-creation and controller-creation calls are asynchronous. The first version waits by pumping the Win32 message loop until the completion handler fires, with a timeout (default 10 s) that returns `Error::Backend`.

### Memory and errors

- Every COM pointer we receive is wrapped in an owning type that calls `Release` on drop. Pointers we pass in are borrowed for the call only.
- `HRESULT` failures become `tpt_shell_core::Error::Backend` with the HRESULT in the message. Failures at the entry point are reported as `Error::Unsupported` when the runtime is missing (the loader reports the version query failure).

## Alternatives considered

1. **Generate bindings from the package's IDL or type library** (`WebView2Interop.idl`, `.tlb` in `build/native/include-winrt`). Less hand-written code and fewer slot errors. Rejected for now because spec §2 and §6.1 say "own COM bindings" and generated code brings a generator dependency. Worth revisiting if the subset grows past about 20 interfaces.
2. **Use the `webview2-com` crate** (MIT, version 0.39.1 seen in the registry). Less work. Rejected because the spec makes the platform layer our code, and the crate adds a large surface we would not control.
3. **Hand-write only the minimal subset and grow it per milestone** (proposed). Matches the spec and keeps unsafe small.

## Test plan

- Vtable order test against `WebView2.h` (described above).
- Handler refcount test: create, AddRef, Release, and check that the box is freed once (uses a counter in the test build).
- Integration test (Windows only, env var `TPT_WEBVIEW2_LOADER` as in `loader.rs`): create an environment, create a controller on a hidden window, load a data-URL page, and check the document title through `DocumentTitleChanged`.
- Leak check: create and destroy the controller 100 times and compare the process handle count.

## Open questions

1. Accept hand-written vtables with the header-order test (alternative 3), or prefer generated bindings (alternative 1)?
2. User-data folder location. Proposed default: `%LOCALAPPDATA%\tpt-shell\<app-id>\webview2`. Do you want a different default, or the profile mapping from spec §10.7 applied now?
3. Hidden-window integration tests on CI runners without a display. Proposed: skip with a message when no interactive desktop is available, and run the full test on a developer machine.

## Decision (2026-10-21)

Decided by following the recommended defaults, as the owner asked:

- Question 1: alternative 3 (hand-written vtables, header-order test). Approved.
- Question 2: default user-data folder `%LOCALAPPDATA%\tpt-shell\<app-id>\webview2`, built by `webview2::default_user_data_dir`, which rejects app ids that are not one plain folder name. The spec §10.7 profile mapping is not applied now; it belongs to M6.
- Question 3: runtime tests skip with a message when `TPT_WEBVIEW2_LOADER` is unset or no interactive desktop exists (`window::runtime_test_loader`). The full set runs on a desktop session, including this machine.
- Deviation: the virtual origin `https://tpt-app.localhost` replaces `app://` on Windows (the first recommendation above). Spec §5.4 and §9 are not edited yet; the wording change is recorded here and in todo.md.

## Implementation status (2026-10-20)

- Implemented: `loader.rs` (typed environment entry point), `com.rs` (GUID, IUnknown, owning `ComPtr`), `handler.rs` (completion handler), `webview2.rs` (environment, controller, webview wrappers), `window.rs` (hidden window and message pump).
- Verified on this machine with runtime 155.0.4283.62: environment, controller and webview creation, navigate to string, and reading the document title back.
- Header check: passes against the NuGet 1.0.4312-prerelease `WebView2.h`.
- Not yet: refcount and leak tests, `app://` serving, `ExecuteScript`, and event subscriptions.
- Deviation: the window handle is passed as an `isize` value so the public API has no raw pointer parameters.

## Status update: messaging and app content (2026-10-20)

- Implemented: `event_handler.rs` (two-object event handler), `WebView::on_web_message`, `WebView::post_web_message`, `WebView::on_web_resource_requested`, `WebView::add_web_resource_requested_filter`, `WebResourceRequest::respond`.
- Verified against runtime 155.0.4283.62: the page posts `ping`, Rust replies `pong`, and the page sets its title to `got-pong`. The test `serves_virtual_host_and_round_trips_a_message` passes.
- **Deviation from spec §5.4 (needs your decision):** `app://` is not routed to `WebResourceRequested`. With the same filter and handler, `https://tpt-app.localhost` works and `app://` produces no requests. The code uses `APP_ORIGIN = "https://tpt-app.localhost"`.
  - Recommended (implemented): the virtual https origin. It is simple and verified. Spec §9's "only app:// loads" becomes "only APP_ORIGIN and allowlisted origins load", enforced by navigation policy (not yet built).
  - Alternative: register `app` as a custom scheme through environment options (`ICoreWebView2CustomSchemeRegistration`). It keeps the name but needs an options COM object, which I have not built.
- Not yet: `embed_dir!` (assets are inline bytes in the test), the JS shim (`window.tpt.invoke`), NavigationStarting policy, refcount and leak tests.

## Status update: navigation policy (2026-10-20)

- Implemented: `WebView::set_navigation_policy`. It uses `NavigationStarting` (slot 4) and `put_Cancel` (slot 5). Top-level navigations are allowed only for `APP_ORIGIN` and `about:blank`. Anything else is cancelled. A URI that cannot be read is cancelled.
- Verified against runtime 155.0.4283.62: `https://example.com/` and `file:///C:/Windows/win.ini` are both cancelled, and the app page keeps its title. Test: `blocks_navigation_outside_the_app_origin`.
- Not yet: `FrameNavigationStarting` (iframes), and redirects are only seen through the top-level event.

## Status update: page runtime and command routing (2026-10-20)

- Implemented: `js/runtime/tpt-shell.js` (`window.tpt.invoke`), injected at document start with `AddScriptToExecuteOnDocumentCreated` (slot 24). `tpt-shell-ipc::Router` parses each page message, dispatches it to a registered handler, and returns the response text.
- Verified: a page calls `window.tpt.invoke('echo', ...)` and gets its arguments back; then `invoke('nope')` and gets `IPC_UNKNOWN_COMMAND`. Test: `shim_invokes_a_rust_command_and_receives_a_typed_error`.
- Size: the runtime is 1245 bytes raw and 661 bytes gzipped (budget in spec §7.2: under 4 KB gzipped).
- Not yet: `tpt.on` for events, capabilities checks in the router, and argument validation against a contract.

## Status update: embedded assets (2026-10-20)

- Implemented: `tpt-shell-macros::embed_dir!`. It expands to `&[(path, content_type, bytes)]`, with paths sorted and `/`-separated. The Windows tests embed `fixtures/app` and serve it from the app origin.
- Verified: the page's `app.js`, served from the embedded set, sets the title. Test: `serves_embedded_assets_from_the_app_origin`.
- Not yet: zstd compression and ETag (spec §5.4), and the rebuild limitation for added files (documented in the macro).
- Policy: `Unicode-3.0` added to `deny.toml` with a justification, because `unicode-ident` comes in through `syn`.

## Status update: leak checks (2026-10-21)

- Handler refcount, drop-once, QueryInterface and owning-pointer tests pass (unit tests in handler.rs and event_handler.rs).
- Controller cycles: live callback objects return to baseline; process handles stable over 15 cycles after warm-up (254 -> 255). Test: controller_cycles_do_not_leak.
- Open: in a 60-cycle run on one environment, creating the 23rd controller did not complete within 30 s (cycles about 9 s each). Cause unknown; investigate before the soak test. Possible workaround under test: a fresh environment per N controllers (not yet tried).

## Status update: controller stall investigation (2026-10-21)

- The stall did not reproduce. Shared environment: 30 controller cycles, no stall, process count flat at 21-23, creation 0.8-5.8 s. Fresh environment per cycle: 10 cycles, no stall, environment creation 0-1.6 s, controller creation 0.7-1.5 s, process count 23-24.
- Cause of the original 60-cycle stall is unknown. Keep the 30 s timeout and treat it as an open risk for the soak test (spec 10.1).
- Creation latency (0.7-5.8 s per controller) is far above the spec 13 cold-start budget. Measure cold start separately before drawing conclusions.

## Status update: cold-start measurement (2026-10-21)

- Five separate-process runs on this machine (warm runtime: about 23 WebView2 processes already running for other apps): loader 0.5-0.7 ms, window 2-10 ms, environment about 6 ms, controller 0.8-2.0 s, first page title 1.3-1.5 s; total 2.1-2.3 s.
- Shell overhead (loader and window) is within the spec 13 budget of 50 ms.
- WebView2 init and first page are 2.1-2.3 s against the 500 ms budget. Not met on this machine. A true cold start (no runtime running) would be slower still, and was not measured because it would close other apps' WebView2 processes.
- The environment figure (about 6 ms) looks too low. Likely the runtime was already up. Not verified.

## Status update: where the cold-start time goes (2026-10-21)

- Navigations in one established controller, 10 pages: first 590 ms, then 360, 290, 278, 296, 283, 280, 293, 278, 271 ms. Steady state about 270-300 ms per page.
- An earlier run of the same test gave 2148, 1061 and 612 ms for its first three pages. So the first navigation after startup is slow and varies with runtime state, while steady-state pages are well inside 500 ms.
- Adopted recommendation (spec 13 budget): the 500 ms first-paint budget is a target to confirm on reference hardware. Local numbers stay on the record here. Spec text unchanged; a change to spec 13 needs your decision.

## Status update: startup overlap (2026-10-21)

- Test: the same 1000 ms of app work either after the webview is ready (sequential) or while the controller is being created (overlap). Ten trials each, separate processes.
- Sequential median 2122 ms (range 1860-2265). Overlap median 2180 ms (range 1054-2342).
- No reliable saving. Three overlap trials were about 1 s faster, the rest matched sequential. Not explained. The async API lets an app start early, but this measurement does not show a gain, so no benefit is claimed.

## Status update: frame navigation policy (2026-10-21)

- Implemented: set_frame_navigation_policy, using add_FrameNavigationStarting (slot 14). The header takes the same NavigationStarting handler as the top-level event, so one shared helper serves both.
- Verified: an iframe to https://example.com is cancelled, the top-level app page still loads, and its title is unchanged. Test: blocks_frame_navigation_outside_the_app_origin.
- Workspace: 44 tests pass; the Windows suite has 15 tests, all passing against runtime 155.0.4283.62.

## Status update: crash callback and reload (2026-10-21)

- Implemented: WebView::on_process_failed (add_ProcessFailed, slot 22) with ProcessFailure::{BrowserExited, RenderExited, Other}, WebView::reload (slot 28). The DevTools call used by the test (CallDevToolsProtocolMethod, slot 33) is crate-private and test-only, so raw protocol access is not offered to pages or agents (spec 9, 10.4).
- Verified: Page.crash through DevTools reports RenderExited; the callback reloads the page and the page loads again. Test: renderer_crash_is_reported_and_the_page_reloads.
- Not yet: BrowserExited handling, and a true process kill (which would close other apps' WebView2 processes on this machine, so it was not done).
- Workspace: 45 tests pass; the Windows suite has 16 tests, all passing.
