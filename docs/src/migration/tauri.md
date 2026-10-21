# Migrating from Tauri

This guide maps a Tauri v2 app onto tpt-shell. It covers what carries over, what changes, and what
the compatibility shim does not do. Claims about Tauri come from its public documentation, cited
below. Claims about tpt-shell come from the code.

Status: the shim `tpt.compat.tauri` is a documented subset and is not finished (todo M11, "finalise
`tpt.compat.tauri`"). The migration is a rewrite of the Rust side plus a small change to the page.
Plan for that, not for a drop-in replacement.

## What is the same

- The UI is still web pages in the system webview. Your HTML, CSS and most JavaScript carry over.
- Commands are Rust functions that pages call and get a promise from.
- Events go from Rust to the page, and the page listens for them.

## What changes

| Tauri v2 | tpt-shell | Notes |
|---|---|---|
| `#[tauri::command]` on a function, registered with `generate_handler!` | `#[command(capability = "...")]`, registered with `register_commands!` in `App::register` | The capability is required in tpt-shell. A command without one is refused on the page path |
| `invoke("name", { camelCaseArgs })` from `@tauri-apps/api/core` | `window.tpt.invoke("name", args)` | See the naming note below |
| Returned values serialised with serde; `Err` rejects the promise | Same idea. Errors are `CommandError` with a stable code | Pages can test `error.code` |
| `emit` and `listen` (`@tauri-apps/api/event`) | `Running::emit(window, name, data)` in Rust; `window.tpt.on(name, handler)` in the page | Events are sent to one window by name. A page-to-Rust `emit` is not in the shim |
| `app.listen` in Rust | Not provided | Use a command, or handle the call in the command's function |
| Capability files in `src-tauri/capabilities/*.json`, with `windows` and `permissions` | Grants in Rust with `Grant { capability, window, origin, scope, consent }` | The default is deny. Scope is part of the grant |
| Plugins (`fs:default`, `dialog:...`) | No plugin system. Write the command and give it a capability | Plugins are not ported |
| `tauri.conf.json` window list | `App::window(Window::new(...))` or `[[windows]]` in `tpt-shell.toml` | The manifest window list is parsed; the running app reads only what the builder sets |
| `window.__TAURI__` (when enabled) | `tpt.compat.tauri.install()` sets `window.__TAURI__` and `__TAURI_INTERNALS__` | Opt-in, as in the shim |

Tauri's documentation says that by default all commands registered with `invoke_handler` can be
called from all windows, and that capabilities restrict plugins. tpt-shell is the other way round: a
page can use a command only when a grant names it.

## Argument names

Tauri documents that arguments are passed as a JSON object with camelCase keys, so a Rust parameter
`invoke_message` is passed from JavaScript as `invokeMessage`. The `#[command]` macro here builds the
schema from the Rust parameter names, and the page must send the same names. If your page sends
`invokeMessage`, rename the Rust parameter to `invoke_message` and send that key, or rename the key
in the page. The shim does not convert names.

## The shim

When `tpt.compat.tauri.install()` runs, the page gets:

- `window.__TAURI__.core.invoke(cmd, args)`, which is `window.tpt.invoke`;
- `window.__TAURI__.event.listen(name, handler)`, which resolves to an unlisten function and calls
  the handler with `{ event, payload }`.

Nothing else from `@tauri-apps/api` is provided. Plugin APIs, window APIs and the Tauri `emit` are
not in the shim.

## Steps

1. Create a tpt-shell project with `tpt-shell new vanilla my-app`, or start a Rust program that
   builds an `App`.
2. Move the front end's files into `App::asset` calls, or `embed_dir!` (see the [Cookbook](../cookbook.md)).
3. For each Tauri command, write a function with `#[command(capability = "...")]` or a hand-written
   handler, and register it.
4. For each capability file, write a `Grant` for the window and the origin the page loads from
   (`https://tpt-app.localhost` on Windows). Pick a scope, and set `consent` where the user should
   decide.
5. Replace `invoke` imports with `window.tpt.invoke`, or call `tpt.compat.tauri.install()` for a
   shim.
6. Check argument names against the Rust parameter names.
7. Replace each plugin with your own command and capability.
8. Run the app on Windows. macOS and Linux are not built (see [Platform notes](../platform-notes.md)).

## What you lose or gain

- You lose: the plugin ecosystem, the mobile targets, and the Tauri updater and bundler. Bundling and
  signing are not built (todo M10).
- You gain: audit of every command call, a default-deny grant model with scopes, consent dialogs in
  native code, an agent surface with policy, and device and kiosk features.

Do not assume parity with Tauri on macOS or Linux, where tpt-shell has no window backend yet.

## Sources

- Tauri v2 "Calling Rust from the frontend": https://v2.tauri.app/develop/calling-rust/
- Tauri v2 "Capabilities": https://v2.tauri.app/security/capabilities/
