# API reference

The public API is documented in the Rust doc comments. This page lists the support status of each
tagged item. The table is generated from the source by `tools/support-tags.py`, so it cannot drift
from the code. Do not edit it by hand.

Every public item must carry a `Support:` line that says `Full`, `Partial` (with the gap) or
`Unsupported` (returns `Error::Unsupported`). See the "Support tags and conformance" section of
[CONTRIBUTING.md](../../CONTRIBUTING.md).

Where an item is not tagged yet, it is listed below as untagged. Those items are not evidence of
support. Read the crate's own module docs, and the [Platform notes](platform-notes.md), before you
rely on them.

## Facade crate

The facade is `tpt-shell`. Its main types:

- `App`: the builder. `new`, `name`, `loader`, `window`, `asset`, `assets`, `grant`, `command`,
  `register`, `profile`, `data_dir`, `audit`, `manifest_audit`, `kiosk_config`, `snapshot_state`,
  `remember_window_state`, `deep_link`, `tray`, `single_instance`, `blob_limit`, `run`, `build`.
- `Window`: `new`, `title`, `size`, `page`.
- `Grant`, `Scope`, `Engine`, `ConsentPrompt`: the permission types from `tpt-shell-capabilities`.
- `Caller`, `Limits`, `Router`, `Target`, `Value`, `CommandError`: the IPC types.
- `command`, `commands`, `register_commands`, `embed_dir`, `include_manifest`, `Contract`: the macros.
- `ProfileKind`, `MenuItem`, `TRAY_ACTIVATE`, `KioskSettings`: smaller types.
- `Error` and `Result`: the error type from `tpt-shell-core`.
- `devices`: the device crate, re-exported as a module.
- `blob`, `Running`: Windows only.

Not re-exported by the facade (use the crate by path): the audit `Policy`, the replay format, the
profile export functions, the agent session and the kiosk config signing helper.

## Support tags

{{#support-tags}}

## Reading the table

- **Full** means the item does what its doc comment says, on the platform the crate builds for and
  with the tests listed in the crate.
- **Partial** names what is missing. Read the note before you use the item.
- **Unsupported** means the call returns `Error::Unsupported`.

Items with a note that names "routed to" a platform crate depend on a backend that is not built for
every OS. Check the [Platform notes](platform-notes.md).
