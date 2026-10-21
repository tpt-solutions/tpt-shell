# Replay format (.tptreplay)

Status: version 1 is implemented in `tpt-shell-replay` (reader, writer, divergence). Capture
hooks in the running app, the player window, compression and assets are not built yet.

## Layout

A recording is UTF-8 text, one JSON object per line.

Line 1 is the header:

```json
{"app":"demo","format":"tptreplay","platform":"windows","version":1}
```

Every later line is one event:

```json
{"data":{"cmd":"ping","id":1},"kind":"ipc_request","seq":0,"t_ns":0}
```

- `seq` starts at 0 and goes up by one per event.
- `t_ns` is the time since the run started, in nanoseconds. It never goes backwards.
- `kind` is one of `ipc_request`, `ipc_response`, `ipc_event`, `input`, `window`, `protocol_load`.
- `data` is the event's payload. Its shape depends on `kind`.

Object keys are written sorted, as the audit log does, so the same recording always produces
the same bytes.

## Versions

The header's `version` must be `1`. A reader that sees any other version refuses the file in
both modes. Future versions will add kinds or fields and bump the version.

## Reading

- **Strict** refuses an unknown kind, a malformed line, a gap in `seq`, or time going backwards.
  Use it in CI and for bug reports.
- **Lenient** skips unknown kinds and malformed lines, counts them in `skipped`, and resyncs
  after a gap in `seq`. Use it to show a recording made by a newer build.

## Divergence

`divergence(recorded, replayed)` compares kind and data event by event and returns the first
difference. Timing is ignored, because it varies between runs. A run that stops early or goes
on past the end of the recording also diverges.

## Limits (R8)

- Timing and random numbers differ between runs unless the app uses the virtualised `Clock` and
  `Rng` (see `tpt-shell-core`). Page-side `Date.now`, timers and `Math.random` need the in-page
  shim, which is not built yet.
- A recording shows what crossed the IPC boundary and what the window did. It does not capture
  the DOM state, so two runs can match on events and still render differently.
- Recordings are not compressed or encrypted yet. Do not share them outside the team until
  redaction hooks exist (`tpt-shell report`, not built).
