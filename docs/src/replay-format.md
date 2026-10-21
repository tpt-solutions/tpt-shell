# Replay format (.tptreplay)

Status: format version 2 is implemented in `tpt-shell-replay` (writer, readers, virtualised
clock and random numbers, timeline model, bug-report bundle, divergence). Not yet built: the
capture hooks inside a running app, the hidden player window, `tpt-shell replay`, and the
page-side shim wired into the event stream. The todo items are in M8 (Record/replay).

The format is specified here. The code is the reference: `crates/tpt-shell-replay/src/lib.rs`
(format and readers), `virtual_time.rs` (clock and random numbers), `codec.rs` (asset bytes) and
`js/replay/shim.js` (page-side shim).

## Layout

A recording is UTF-8 text, one JSON object per line (JSON Lines). There is no compression and no
encryption (see [Why plain text](#why-plain-text)).

Line 1 is the header:

```json
{"app":"demo","format":"tptreplay","platform":"windows","version":2}
```

Every later line is either an **asset** (version 2 only) or an **event**.

Events:

```json
{"data":{"id":1,"cmd":"ping"},"kind":"ipc_request","seq":0,"t_ns":0}
```

- `seq` starts at 0 and goes up by one per event.
- `t_ns` is the time since the run started, in nanoseconds. It never goes backwards.
- `kind` is one of the eight kinds below.
- `data` is the payload. Its shape depends on `kind`.

Object keys are written in sorted order, so the same recording always produces the same bytes.

### Event kinds

| Kind | Meaning |
|---|---|
| `ipc_request` | A call from a page to Rust |
| `ipc_response` | The reply Rust sent back |
| `ipc_event` | An event Rust sent to a page |
| `input` | A DOM input event, with a stable target path |
| `window` | A window event, such as resize, theme or focus |
| `protocol_load` | A custom-protocol load (`app://`) |
| `clock` | One reading of the virtualised clock. `{"op":"mono"\|"wall","ns":n}`; the page shim adds `{"op":"advance","ms":n}` |
| `rng` | One draw from the virtualised generator. `{"value":n}`; the page shim adds `{"op":"js-random","value":n}` |

Recorders refuse any other kind, so a recording never holds a kind that the strict reader would
reject.

### Assets (version 2)

An asset is a file the run used, such as an image the page loaded. It is stored inline as base64
(RFC 4648, standard alphabet, with padding):

```json
{"asset":{"data":"aGVsbG8=","len":5,"name":"logo.png"}}
```

- `name` is 1 to 100 ASCII letters, digits, `-`, `_` or `.`. It cannot be `.` or `..`, and it
  never carries a path, so an unpacked asset cannot be written outside its directory.
- `len` must equal the decoded byte count.
- The limit is 16 MiB per asset (`MAX_ASSET_BYTES`).

## Versions

| Version | Written by | Read by | Notes |
|---|---|---|---|
| 1 | no | yes | No assets, no `clock` or `rng` events |
| 2 | yes | yes | Adds assets, `clock` and `rng` events |

A reader that sees any other version refuses the file in both modes. A future version will add
kinds or fields and bump the number.

## Reading

- **Strict** refuses an unknown kind, a line that is not JSON, a gap in `seq`, time going
  backwards, a bad asset, or an asset in a version 1 file. Use it in CI and for bug reports.
- **Lenient** skips unknown kinds, malformed lines and bad assets, and counts them in `skipped`.
  After a gap in `seq` it resumes from the new number. Use it to show a recording made by a newer
  build.

An empty file and a header that is not `tptreplay` are refused in both modes.

## Divergence

`divergence(recorded, replayed)` compares kind and data event by event and returns the first
difference, with its index and the expected and actual values. Timing is ignored, because it
varies between runs. A run that stops early or goes on past the end of the recording also
diverges.

Inside a replay session, a clock or random read that does not match the recording (wrong kind,
wrong operation, or none left) is noted as the session's divergence. The read returns the last
value it had, so the run keeps going and the first difference can be reported.

## Virtualised time and random numbers

**Rust side** (`tpt-shell-replay::virtual_time`). When recording, a virtual clock reads the real
clock it wraps and logs each reading as a `clock` event. A virtual generator does the same for
`rng` events. When replaying, neither touches the real source: each read takes the next recorded
value, in order. The clock and random events share one ordered stream, so the app must read them
in the same order in both runs.

**Page side** (`js/replay/shim.js`). The shim is installed only in record or replay runs. It
replaces:

- `Date.now()`, which returns a virtual time in milliseconds;
- `performance.now()`, which counts from the start of the run;
- `Math.random()`, a seeded generator (mulberry32). The same seed draws the same numbers;
- `setTimeout`, `setInterval` and their `clear` functions. Timers run in order of due time, then
  creation order.

The virtual clock moves only when the host calls `advance(ms)`. A recording calls it with the
real elapsed time and logs each amount. A replay calls it with the recorded amounts, so timers
fire at the same virtual times. One `advance` may fire at most 10,000 timers; more is an error,
so a looping timer cannot hang the host.

```js
const shim = TptReplayShim.install({ seed: 42, start: 0, onEvent: (kind, data) => log(kind, data) });
shim.advance(16);
shim.restore();   // puts the real Date.now, Math.random and timers back
```

## Limits (R8)

This section is the determinism review for risk R8 in `spec.txt` ("replay determinism is bounded
by engine behaviour"). It was done by reading the code of `tpt-shell-replay`, `js/replay/shim.js`
and the platform crates' event paths. It was not done by running recordings on each OS, so the
acceptance test in spec §10.5 (100 strict replays of the sample app on the recording OS) is still
open.

What a replay does **not** control:

- **`new Date()` is not virtualised.** Only `Date.now()` is. Code that builds a date from the
  constructor reads the real clock. Use `Date.now()` in code that must replay.
- **Promises are not virtualised.** Their ordering is set by the engine's microtask queue. The shim
  does not change it.
- **`requestAnimationFrame` is not virtualised.** Frame callbacks run on the engine's own
  schedule. An app that drives its state from frames will not replay exactly.
- **Engine-internal timing is not captured.** Layout, paint, GPU work, font loading and garbage
  collection take different times on each run. Their timing does not reach the recording.
- **Network traffic outside `app://` is not captured.** Responses from other hosts can differ
  between runs.
- **The DOM state is not recorded.** A recording holds IPC traffic, input and window events, and
  protocol loads. Two runs can match on events and still render differently. The devtools
  time-travel view can attach DOM snapshots, but the capture hook that takes them is not built.
- **Timing differences are ignored by design.** Divergence compares kind and data, not `t_ns`.
- **Cross-OS replay is lenient.** Different platforms give different IPC responses and window
  events. Expect divergence reports, not a pass.

What the format does do:

- A divergence report gives the first point where kind or data differ, so a failure has a place
  to start.
- Strict mode stops on the first unknown or malformed line, so a damaged file is not replayed as if
  it were complete.
- Lenient mode lets an older reader show a newer recording, and it counts what it skipped.

Mitigations in code today: the Rust clock and generator are virtualised; the page shim is opt-in
and off in normal runs; the recorder refuses unknown kinds and time going backwards.

## Why plain text

The product spec (§10.5) names zstd compression. The format does not use it. A zstd reader and
writer would add a dependency that has to pass the licence policy, and the recordings are small
enough for CI artefacts and bug reports. Plain JSON Lines can be read with any text tool, which
also helps review. If recordings grow large, compression can come in a later format version. Until
then, do not share a recording outside the team: it holds IPC arguments and asset bytes unless
redacted. The redaction hooks in `tpt-shell-replay::report` (used by `tpt-shell report`, not yet
built) are the planned control.
