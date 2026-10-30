# tpt-shell-profiles

Profiles and ephemeral sessions for tpt-shell. A profile is the folder where an app keeps its window data, such as the web storage and cookies a page creates. This crate names those folders, keeps them apart, wipes them, caps their size, and exports them encrypted.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). Wiping is best effort, and the documentation says so.

## Kinds of profile

| Kind | Behaviour |
|---|---|
| `Kind::Default` | The app's default folder. |
| `Kind::Named(name)` | A named profile that persists between runs. |
| `Kind::Ephemeral` | A temporary folder that is wiped when the app stops. |

## What it provides

- `ProfileStore`: creates, lists and deletes named profiles under one base folder. Names are checked, and every path is built from a checked name, so a profile cannot reach outside the store.
- `Ephemeral`: a temporary session folder, wiped when it is dropped or closed.
- `wipe_dir`: best-effort overwrite and delete.
- `usage` and `enforce_quota`: the size of a profile, and a cap on it.
- `export`: encrypted export of a profile folder. The cipher is ChaCha20-Poly1305. The key comes from PBKDF2-HMAC-SHA256 with 600,000 iterations.

## Limits of wiping

`wipe_dir` overwrites a file before it deletes it. SSDs and journaling file systems can keep old copies that software cannot reach, so the wipe is not a guarantee. Use an encrypted volume where that matters.

## Usage

```rust
use tpt_shell_profiles::check_name;

assert!(check_name("work").is_ok());
```

In an app, the facade picks the folder with `App::profile(ProfileKind::Named(..))` or `ProfileKind::Ephemeral`.

## Platform support

| Part | State |
|---|---|
| Folder mapping, names, wipe, quota and export | Platform-neutral, tested on every host. |
| WebView2 profile mapping (Windows) | Built in the Windows backend. The Windows crate shares one folder for all windows. Per-window profiles are not built. |
| macOS profile isolation | Needs macOS 14 or later. Not run on a Mac yet. |
| Linux | Not built here. |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
