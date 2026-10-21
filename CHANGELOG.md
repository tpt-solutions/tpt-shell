# Changelog

All notable changes to tpt-shell are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The project will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Repository skeleton: Cargo workspace, empty facade crate `tpt-shell`.
- Dual licence: MIT and Apache-2.0 (TPT Solutions).
- Licence and advisory policy in `deny.toml`.
- Governance: SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md, this changelog.
- CI skeleton: fmt, clippy, test on three operating systems, cargo-deny, SPDX header check.
- `tpt-shell audit verify | export | view`: checks the hash chain (and checkpoint signatures with `--key`), exports verified logs as CSV or JSON Lines, and prints a short table.
- `[audit]` manifest section: `store_params` and `redact`, validated against the published schema.
- Windows: pages get `window.tpt` (the page runtime) in every window. Before this it was only added by tests.
- Windows: the high contrast, reduced motion and reduced transparency settings reach the page as `data-` attributes. Open pages update them, and the colour scheme, when the user changes a setting.
- `BENCHMARKS.md` with the method and the hello-world binary size (690 KiB, budget 4 MB).
- `tpt-shell-kiosk` (new crate): a kiosk config that loads only with a valid Ed25519 signature, atomic state snapshots that restore on the next start, and an idle timer. The app does not use them yet.
- `tpt-shell-supervisor`: runs the app as a child process with a heartbeat file, stops a run that stops beating or reaches its scheduled restart, restarts with backoff, gives up after a crash loop, and writes an alert (command or file) and a rotating log.
- Devices: the transport trait, a hub that opens a device only under a scope and audits opens, writes and closes, and virtual devices (serial loopback, printer with a receipt preview, scanner, HID) for tests.
- `tpt-shell install-service <name> <program>` (and `uninstall-service`) starts a program at sign-in, for the current user (Windows).
- `App::manifest_audit(manifest, log)` writes command calls with the `[audit]` section of the manifest.
- Kiosk: `App::kiosk_config` applies a signed config (home page, idle reset). `App::snapshot_state` saves app state on a timer and at exit, and restores it at the next start.
- Devices: `tpt_shell::devices` re-exports the device crate. A page command with a device grant can print through the hub; live test with a virtual printer.
- Windows: timers on the UI thread (`WindowsBackend::every`, `cancel`).
- CLI: `tpt-shell dev <dir> [--port N]` serves an app's files on 127.0.0.1 and reports which files change.
- `App::profile(ProfileKind::Named(..) | Ephemeral)` picks the folder the windows keep their data in. An ephemeral session is wiped when the app stops.
- Agent: the page's accessible tree as a snapshot (roles, names, states, values, bounds, references), and the actions click, focus, type, select, key, scroll and read. Password values and `data-tpt-redact` blocks are never reported. Page text is wrapped in `<untrusted>` markers when rendered. Live test on WebView2.
- Windows: keystore with DPAPI (per user, one encrypted file per name); balloon notifications from the tray icon; deep-link registration (`App::deep_link`); window placement saved and restored (`App::remember_window_state`).
- Windows: `App::tray(tooltip, items, on_select)` adds a notification-area icon. A left click reports `TRAY_ACTIVATE`, and a right-click menu pick reports the item's id.
- Windows: `App::single_instance(id, on_forwarded)`. A later launch hands its arguments to the first instance and exits. `tpt_shell_platform_windows::single_instance` does the work (named mutex and `WM_COPYDATA`).
- The page runtime has `tpt.on(event, handler)`, which returns an unsubscribe function, and `tpt.caps` (engine `webview2` and its version).
- `App::audit(path, policy)` writes each command call from a page to a hash-chained audit log: window, command, capability, arguments (or only their hash), and outcome.
- `tpt_shell::blob::put_blob(media_type, bytes)` (Windows): stores bytes and returns an app URL a page can `fetch`. Ids are random 128-bit values, and the store holds at most 64 MiB. `remove_blob` releases one.
- `Router::capability_of(cmd)`: the capability a guarded command needs, used by the audit record.
- `Running::emit(window, name, data)` sends an event to a window's page, which receives it through `tpt.on`. Events are not queued.
- `[audit]` settings map onto the audit writer's policy (`Policy::from(&AuditSettings)`). No runtime writer uses the policy yet.
- Command errors can carry structured `details`; invalid arguments report the offending path.
- MSRV is Rust 1.90, set in the workspace manifest and measured on 1.90.0.
- Windows: release builds turn off the developer tools and the right-click menu for each page window (debug builds keep them on).
- Windows: every page the shell serves carries a strict Content-Security-Policy (`script-src` and `style-src` limited to the app's own origin; inline script and style are blocked).
- `[[windows]] allow_navigation`: `https://` URL prefixes a window may navigate to. Remote navigation is blocked otherwise (Windows).
- Windows: a window can receive messages from its page (`set_message_handler`). The handler's reply is posted back to the page. A runtime test runs a capability-checked command through it.
- Windows: app pages are served from the app origin (`https://tpt-app.localhost`) from a window's asset source, so grants match the page's real origin. The `data:` page path is kept only for shell-internal use.
- `#[command(capability = "fs.read", target(path = "file"))]`: the grant's scope checks the named argument (path, device, url or bytes).
- `#[command]`, `commands!`, `register_commands!` and `embed_dir!` are re-exported from `tpt_shell`. A command with `capability = ...` registers behind its grant, so it cannot run through the unauthorized entry point.
- Windows: a grant that needs consent asks the user in a native message box owned by the requesting window. The default answer is No.
- `tpt_shell::App` (Windows): `App::new().loader(...).asset("index.html", "text/html", ...).window(Window::new("main")).grant(...).command(...).run()`. Pages call guarded commands through the checked router; a grant that needs consent asks in a native dialog.
- IPC: commands can be registered behind a capability (`register_guarded`). `handle_authorized` checks the caller's window and origin grant before the handler runs, and asks for consent when a grant needs it. Guarded commands are refused on the unauthorized entry points.
- Capabilities: `Scope::PathGlobs` matches paths by pattern. `*` and `?` match within one segment, and `**` matches any depth. Patterns are normalized, and a bad one fails when the grant is added.
- Capabilities: `FileConsent` keeps consent answers in a file, so a remembered answer survives a restart. It is not yet placed in the profile folder by the app.
- Audit: `Signing::every_secs` writes a signed checkpoint after an entry once that many seconds have passed since the last checkpoint, alongside the entry-count interval.
- CLI: `tpt-shell contract diff <old.json> <new.json>` lists each change as breaking or compatible for a caller built against the old contract. `contract check <new.json> --baseline <old.json>` lists only the breaking changes and exits 1 when there are any. The comparison is `tpt_shell_ipc::contract_diff`.
- App: consent answers are kept in `consent.txt` in the window's data folder. A named profile keeps them, and an ephemeral session drops them when it ends.
- `tpt-shell-test` (new crate): a screenshot comparer over raw RGBA, with a YIQ-weighted distance, ignore and per-region tolerance regions, and a changed-share threshold. PNG decoding and the CLI are not built.
- Docs: the five-minute quickstart covers the hello window (`examples/hello`), scaffolding, the manifest check and the dev preview. Each command was run on Windows.
- CLI: `tpt-shell dev` prints the served folder without the Windows `\?\` prefix.
- `tpt-shell-test::a11y`: contrast ratio with the WCAG AA minimums and the large-text rule, a 24 px target-size check, and unnamed form inputs.
- Audit: a seeded property test over signed chains with checkpoints. Byte flips, removed records and swapped records are all caught.
