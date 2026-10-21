# Migrating from Electron

This guide maps an Electron app onto tpt-shell. The main difference: an Electron app runs Node.js
in its main process, and tpt-shell runs Rust there. Your page code carries over. Your Node code moves
to Rust commands with grants.

Claims about Electron come from its public security documentation, cited below. Claims about
tpt-shell come from the code.

Status: there is no Electron compatibility shim. The `tpt.compat.tauri` shim is for Tauri-style
calls only. Plan a rewrite of the main-process code.

## Security defaults

Electron's security tutorial and its context isolation tutorial say:

- Context isolation is on by default from Electron 12.0.0.
- Node.js integration in renderers is off by default from Electron 5.0.0.
- Process sandboxing is on by default from Electron 20.0.0.
- `contextBridge.exposeInMainWorld` is how a preload script exposes an API to the page.

tpt-shell has no Node.js in pages. A page sees only `window.tpt`, and a call from it goes through the
router, with grants and an audit entry when audit is on. The page cannot reach files, processes or
the network except through a command with a grant.

## Mapping

| Electron | tpt-shell | Notes |
|---|---|---|
| Main process (Node.js) | The Rust program that builds `App` | Node packages do not run; port the logic to Rust |
| `ipcMain.handle(channel, fn)` | `#[command]` or `App::command` | Each command names a capability |
| Preload script with `contextBridge.exposeInMainWorld` | `window.tpt.invoke` and `window.tpt.on`, provided by the runtime | No preload file is needed |
| `ipcRenderer.invoke(channel, args)` | `window.tpt.invoke(name, args)` | Argument names must match the Rust parameter names |
| `webContents.send(channel, data)` | `Running::emit(window, name, data)` | Sent to one window by name |
| `ipcRenderer.on(channel, fn)` | `window.tpt.on(name, fn)` | |
| `BrowserWindow` options | `Window::new(name).title(...).size(w, h)` | Placement and frameless options are partly built (see the table in [Platform notes](../platform-notes.md)) |
| `fs`, `child_process`, `net` in main | A command with a capability and a scope | Grant the smallest scope |
| `dialog.showOpenDialog` | The Windows platform layer has open, save, message and folder dialogs. Pages cannot call them yet | Call them from a command in Rust |
| `Notification` | `Running::notify(title, body)` on Windows | Other platforms not built |
| `app.setAsDefaultProtocolClient` | `App::deep_link(scheme)` | Windows only |
| `app.requestSingleInstanceLock` | `App::single_instance(id, handler)` | Windows only |
| `Tray` | `App::tray(tooltip, items, handler)` | Windows only |
| `safeStorage` | The DPAPI keystore on Windows | Other platforms not built |
| `session.fromPartition` | Profiles: `App::profile(ProfileKind::...)` | Windows only |
| `electron-builder`, auto-update | Not built. Bundling and signing are open (todo M10) | |

## Steps

1. Inventory the main-process code. List each `ipcMain.handle` channel and each Node module it uses.
2. Port each channel to a Rust function. Give it a capability name, and write the grant with the
   window, origin, scope and consent you want.
3. Replace each `ipcRenderer.invoke` with `window.tpt.invoke`, and check the argument names.
4. Remove the preload script. Replace each `ipcRenderer.on` with `window.tpt.on`.
5. Move any Node-only libraries to Rust crates, or to a separate process, and call them from a
   command. Check their licences against `deny.toml` (MIT, or dual with MIT).
6. Check the page's use of browser APIs. The page is still in the system webview, so `fetch` to other
   origins is blocked by the page policy. Route those calls through a command, with a URL scope.
7. Run on Windows. macOS and Linux windows are not built.

## What you lose or gain

- You lose: Node packages in the main process, the Electron tooling and update flow, and the
  Chromium-version control that Electron gives you. The shell uses the system webview (WebView2 on
  Windows), which varies by machine.
- You gain: default-deny capabilities with scopes, an audit log of command calls, a policy-checked
  agent surface, and no Node.js in the page.

Electron's page isolation is a bundled setting. In tpt-shell the page policy is fixed and checked on
every response. Test your pages against it. Inline scripts and styles do not run.

## Sources

- Electron "Context Isolation": https://www.electronjs.org/docs/latest/tutorial/context-isolation
- Electron "Security": https://www.electronjs.org/docs/latest/tutorial/security
