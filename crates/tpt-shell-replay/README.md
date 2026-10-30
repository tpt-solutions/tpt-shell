# tpt-shell-replay

The `.tptreplay` format for tpt-shell: a recording of what happened in an app run, which can be read back, played against a target, scrubbed through in time, and packed into a bug-report bundle with sensitive data redacted.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell). The format is also described in [docs/src/replay-format.md](https://github.com/tpt-solutions/tpt-shell/blob/master/docs/src/replay-format.md).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The format, readers, recorder and divergence check are Full. The capture hooks in the running app are Partial.

## The format

A recording is JSON Lines.

- **Line 1 is the header:** `{"format":"tptreplay","version":2,...}`.
- **An `asset` line** carries a file the run used (version 2 only). Its SHA-256 is checked on read.
- **Every other line is one event:** `{"seq":n,"t_ns":t,"kind":"...","data":{...}}`.

`seq` counts from 0 and `t_ns` never goes back.

Version 2 adds assets and the clock and random-number kinds. Version 1 recordings still read, and have no assets. Recordings can be compressed as a zstd frame with `Recorder::finish_compressed`, and read back with `read_compressed`. Plain text still reads with `read`.

A recording is limited to 256 MiB, and an asset to 16 MiB.

## Readers

| Mode | Behaviour |
|---|---|
| `Mode::Strict` | Refuses anything it does not understand: unknown kinds, malformed lines, gaps in `seq`, or time going backwards. |
| `Mode::Lenient` | Skips unknown kinds and malformed lines, and counts them. An older reader can then show a newer recording. |

## What it provides

| Module | Contents |
|---|---|
| root | `read`, `read_compressed`, `Header`, `Event`, `Asset`, `Replay`, `Mode`, `Recorder`, `divergence` and the constants `VERSION`, `READABLE_VERSIONS` and `KINDS`. |
| `codec` | The text and zstd encoding. |
| `capture` | One typed helper per event kind, for the recording side. |
| `virtual_time` | Makes the `Clock` and `Rng` traits from `tpt-shell-core` replayable. |
| `player` | Runs a recording against a target. The reference target is the echo target used by the CLI. |
| `timeline` | The data model for time-travel: the state at any point in the run. |
| `report` | The bug-report bundle, with redaction. |

`divergence` compares a recording with the events of a new run. It compares kind and data, and ignores timing, which varies from run to run.

## Usage

Read a recording:

```rust
use tpt_shell_replay::{read, Mode};

let text = std::fs::read_to_string("run.tptreplay")?;
let replay = read(&text, Mode::Strict)?;
```

From the command line:

```sh
tpt-shell replay run.tptreplay --strict
tpt-shell report bug.json --replay run.tptreplay --log app.log --redact "C:\Users\name"
```

`--redact` removes each given string from the bundle.

## Platform support

The format, readers and player are platform-neutral. The capture hooks are wired into the Windows backend. macOS and Linux do not capture yet.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
