# RFC 0001: WebView2 COM layer (Windows backend)

Status: **Proposed.** Awaiting review. No COM code has been written yet.
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
- Each vtable is checked by a test that reads `WebView2.h` from the package and asserts that method names appear in the same order as in our declaration. This catches reordering, which would otherwise cause a crash at runtime. The test runs in CI only when the header is available, and is skipped with a message otherwise.

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

## Decision needed

Approve the design (alternative 3) and the defaults above, or change them. Implementation starts after approval.
