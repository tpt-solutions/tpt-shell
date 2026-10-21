# Concepts

This page explains the parts of tpt-shell and how they fit together. The API is in
[API reference](api.md). The code samples are in the [Cookbook](cookbook.md).

Status: the Windows backend runs apps today. macOS and Linux are cross-compiled only, and
`App::run` returns `Error::Unsupported` there. See [Platform notes](platform-notes.md).

## The app

An app is a set of web pages in the operating system's own webview, plus Rust code. The Rust
code is the trusted side. The pages are untrusted: anything a page sends is checked before Rust
acts on it.

You build an app with `App`. It collects windows, files, grants and commands. Nothing starts
until you call `App::run`, which blocks until the last window closes or the app quits.

## Windows

A `Window` has a name, a title, a size and a page. The name is its identity: grants name the
window they apply to, so two windows never share permissions by accident.

Each window loads its page from the app origin (`https://tpt-app.localhost` on Windows). Files
you add with `App::asset` are served from that origin.

## Assets and the page policy

Pages are served with a Content-Security-Policy that allows only the app's own scripts and
styles. Inline `<script>` and `<style>` blocks do not run, and remote origins are blocked. Put
scripts and stylesheets in files and add them with `App::asset`.

## Commands

A command is a Rust function a page can call. A page calls it with `window.tpt.invoke(name, args)`
and gets a promise. The call is an envelope:

```json
{"v":1,"id":1,"cmd":"list_dir","args":{"path":"C:/data"}}
```

Before the handler runs, the router checks:

1. the envelope (version, size, depth, field names);
2. the arguments against the command's schema, when the command has one (unknown fields are
   refused);
3. the capability, and the scope of the target the arguments name.

A handler never sees input that fails these checks. Errors come back with a stable code, such as
`CAP_NO_GRANT` or `IPC_INVALID_ARGS`.

Two ways to make a command:

- `#[command]` on a Rust function, with `commands!` or `register_commands!` to register it. The
  macro builds the argument schema from the function's types (see the
  [Cookbook](cookbook.md)).
- `App::command(name, capability, target_of, handler)` for a handler written by hand. Use it when
  the command acts on a path, URL or device, so the scope check has a target to read.

## Capabilities and grants

A capability is a named permission, such as `fs.read`. The default is deny. A page can use a
capability only when a **grant** names it, for one window and one origin:

- `Scope::Paths` limits a request to directories below the given roots.
- `Scope::PathGlobs`, `Scope::Devices`, `Scope::Urls` and `Scope::MaxBytes` limit it in other ways.
- `Scope::Unrestricted` allows any target. Use it sparingly.
- `consent: true` asks the user once, in a native dialog that the page cannot see. The default
  answer is No.

A grant for one window does not apply to another window. Path requests are normalised before the
check, and a path with `..` is refused, so it cannot climb out of a root. Relative paths, UNC
paths and reserved device names are refused too.

## The audit trail

When you call `App::manifest_audit` (or `App::audit`), each command call from a page is written to
an append-only log. Each record holds the time, the window's actor, the command, the capability,
the outcome, and a SHA-256 hash of the parameters. Records are chained: each one holds the hash of
the record before it. A later edit, a dropped record or a reordering breaks the chain, and
`tpt-shell audit verify` reports the first bad sequence number.

The chain cannot show that the end of the file was cut off, or that the whole file was replaced.
Signed checkpoints and external anchors close that gap. See [Audit](guides/audit.md).

## Profiles and sessions

A window keeps its data (WebView2 profile, cookies, local storage) in a folder. `App::profile`
chooses the folder:

- the default folder under `%LOCALAPPDATA%\tpt-shell\<app>`;
- a named profile, which lasts until it is deleted;
- an ephemeral session, which uses a new folder that is deleted when the app stops.

Every window of one app shares the chosen folder. Per-window profiles are not built. See
[Profiles](guides/profiles.md).

## Jobs and events

A **job** is a long-running command. It runs on a background thread, reports progress and can be
cancelled. The page gets a job id back and listens for `job.*` events. The JavaScript runtime has
`tpt.job(id)` for this.

An **event** is a message from Rust to a page, sent with `Running::emit(window, name, data)`. Pages
listen with `tpt.on(name, handler)`.

Support: the job runner is in `tpt-shell-ipc::jobs`. Its API is in the [API reference](api.md).

## Devices

Devices (serial ports, HID devices, printers, network devices) are opened through a hub. The hub
checks a device scope before it opens anything, and it audits each open, write and close. Virtual
devices let you test without hardware. See [Devices](guides/devices.md).

## Kiosk

A kiosk config is a signed JSON file. The app loads it only when its Ed25519 signature checks out.
The config sets the home page and the idle time. OS-level lock-down is the operator's job; the
shell does not lock the whole desktop. See [Kiosk deployment](guides/kiosk.md).

## Agents

An agent is an automated client that drives the app's pages through a fixed set of tools, under a
policy with rate limits, a route allow-list and confirmation steps. The agent surface has no way to
run JavaScript. See [Agents](guides/agents.md).

## Errors

Every error has a stable `TS####` code for manifest, audit, agent, test and licence problems, and a
fix hint. `tpt-shell explain TS0002` prints the entry. IPC and capability refusals use codes such as
`IPC_INVALID_ARGS` and `CAP_OUT_OF_SCOPE`. The list is in [Troubleshooting](troubleshooting.md).

## Telemetry

tpt-shell has no telemetry. It sends no usage data anywhere, and it opens no sockets unless you
enable a feature that needs one.
