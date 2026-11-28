# Changelog

All notable changes to `tpt-shell` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- `App::from_manifest(path)` builds the app's name, windows (`[[windows]]`, with their `allow_navigation`) and virtual devices (`[devices]`) from the manifest. Pages, commands, grants and the agent profile stay in code.
- `Window::allow_navigation(prefixes)` sets the `https://` prefixes a window may navigate to, besides its own pages (Windows).
- Feature `net-devices` (off by default) turns on raw TCP to network devices through the app.
- Windows: `App::audit_signing_key(name)` signs the audit log's checkpoints with a key kept in the DPAPI keystore. The first run makes the key and writes its public half to `<log>.pub`; later runs reuse it (Partial: Windows only).
- `App::manifest_devices(path)` opens the virtual devices named in the manifest's `[devices]` section (`virtual = true` and `virtual_devices = ["kind@id", ...]`). A non-empty `TPT_VIRTUAL_DEVICES` replaces the list for that run.
- `App::assets_compressed(files)` takes the output of `embed_dir_compressed!` and decompresses each file once at `build`. A file that does not decompress fails the build and names the file.
- Windows: `Window::on_monitor(index)` and `Window::on_each_monitor()`. Monitors are numbered from 0 in device-name order, and a window stays on its monitor when the layout changes. `on_each_monitor` opens one window per monitor, named `<name>-1`, `<name>-2`, and so on. An index with no monitor behind it stops the app before anything opens.
- Windows: `Window::hidden()`. The page still loads and runs, so a test can drive the app with nothing on screen (Partial).
- Windows: `Window::profile(ProfileKind)` keeps a window's data in its profile's folder. Windows that share a profile share that folder, and their consent answers are kept there too.
- Windows: `App::menu_bar(menus, on_command)` gives every top-level window a menu bar, and windows created later get it too. Ids must be non-zero.
- Windows: `App::webview2_channel(...)` selects the Stable, Beta, Dev or Canary runtime (Partial).
- Windows: `App::agent_http(port)` serves MCP on `127.0.0.1` behind a bearer token from `TPT_MCP_TOKEN` (feature `mcp-http`).
- `App::replay_recording` plays a recording's requests through the app's own router, with no window. `App::replay_page_input` replays a recording's page input on Windows (feature `agent`).
- Features `mcp-http` (MCP over localhost HTTP) and `test-driver` (answers the test host's requests on the UI thread, when the app is started with `TPT_TEST_RUN`).
- `tpt_shell::viewport` and `UiHandle::viewport_frame`: sends one RGBA8 frame to a window's page as the event `viewport.frame`, for a `<canvas>` to draw. The payload is JSON (`width`, `height`, `format: "rgba8"`, base64 `data`). A buffer that is not `width * height * 4` bytes, or an empty frame, is refused.
- `UiHandle::viewport_frame_named`: the same frame with a binding `name`, so a page with several canvases draws each frame into the right one.
- `examples/viewport`, which streams an animated gradient into a page canvas. The page's script and style are assets, because the page policy blocks inline code.
- Facade crate with the `App`, `Window`, `Scope` and command types, and re-exports of the core, IPC, capability, audit, profile and macro crates.
- Windows: `App::new().loader(...).asset(...).window(...).grant(...).command(...).run()` opens a window, serves its pages from the app origin, and runs capability-checked commands. A grant that needs consent asks in a native dialog, and the default answer is No.
- Windows: `App::audit(path, policy)` writes each command call from a page to a hash-chained audit log. `App::manifest_audit(manifest, log)` does the same from the `[audit]` section of the manifest.
- Windows: `App::tray`, `App::single_instance`, `App::deep_link`, `App::remember_window_state`, the keystore and balloon notifications.
- Windows: `put_blob` and `remove_blob` for binary data a page can fetch, with a 64 MiB store limit.
- Windows: `Running::emit` sends an event to a window's page, which receives it through `tpt.on`.
- `App::profile(ProfileKind::Named(..) | Ephemeral)` picks the folder a window keeps its data in. An ephemeral session is wiped when the app stops.
- `App::kiosk_config` applies a signed kiosk config. `App::snapshot_state` saves and restores app state.
- `App::contract` and `App::write_contract`, and the `--tpt-write-contract <path>` flag.
- Features `agent`, `admin-endpoint`, `health-endpoint` and `devtools`. All are off by default.
- `app://__tpt/licenses` serves the licence page when one is embedded at build time.
- `examples/hello`, which opens one window with one embedded page.
- Release builds turn off the developer tools and the right-click menu on Windows. Debug builds keep them on.
- Every page the shell serves on Windows carries a strict Content-Security-Policy.

### Changed
- A kiosk's navigation list replaces a window's list only when the kiosk sets one. Before, it always replaced it.
- The agent's window (the first window) may navigate only to the agent profile's routes, narrowed by a kiosk list. Its other script navigations are cancelled.
- MSRV is Rust 1.90, set in the workspace manifest.
- The README quickstart no longer calls `App::loader`, which is optional on Windows.
- The quickstart uses the vendored `redist/win-x64/WebView2Loader.dll`, and `tpt-shell run` copies it beside the program.

### Fixed
- Agent: a click on a link, or on a button that submits a form, is checked against the profile's routes before it runs. The page is asked where the click would go first; a destination outside the routes refuses the click and it does not run.
- Agent: `wait_for` keeps polling when a snapshot fails while the page loads, and reports the last failure only at its timeout.
- Agent: a profile's `actions` list now decides which page tools (`click`, `type_text`, `read`, `wait_for`) an agent is offered. Before, `invoke` alone offered all of them, and a profile without `invoke` offered none. Typed commands still need `invoke` and a listed name.
- Agent: two agent tools with the same name (a typed command and a page action, say) now stop the app at start with a message naming the tool. Before, the session silently answered with the first one.
- Windows: job events go only to the window that started the job.
- Windows: UI Automation reads of a new window retry instead of failing with `E_FAIL`.
