# Profiles and sessions

This guide covers where a window keeps its data: the default folder, named profiles, and ephemeral
sessions. It also covers export, quotas and wiping. It is for developers who choose the storage,
and for operators who need to know what is left on a machine.

Status: the profile kinds, the profile store, ephemeral folders, wiping, quotas and encrypted export
are built and tested as file operations. The app uses the chosen folder for its windows on Windows,
and each window can have its own profile there. The browser-side mappings on macOS and Linux, and
the conformance tests for isolation, are not done.

## Kinds

`ProfileKind` (re-exported by the facade) has three values:

- `Default`: the app's default folder, `%LOCALAPPDATA%\tpt-shell\<app>\profiles` for a named or
  default profile.
- `Named(name)`: a profile that lasts until it is deleted.
- `Ephemeral`: a new folder in the temporary directory. It is deleted when the app stops.

```rust,compile-check
use tpt_shell::{App, ProfileKind};

fn private_session() -> App {
    App::new()
        .name("kiosk-demo")
        .profile(ProfileKind::Ephemeral)
}
```

`App::data_dir(path)` names a folder directly. It wins over the profile.

Every window of one app shares the chosen folder unless the window says otherwise. A window can
name its own profile with `Window::profile(kind)` (Windows):

```rust
App::new("demo")
    .window(Window::new("main"))                                   // the app's folder
    .window(Window::new("work").profile(Kind::Named("work".into()))) // its own folder
```

Windows that name the same profile share one WebView2 environment, so they share their storage.
A window in another profile cannot see cookies, local storage or cache from the first. A window
with `Kind::Ephemeral` gets a session folder that is wiped when the app stops. A profiled window's
consent answers go in that folder's `consent.txt`; other windows keep theirs in the app's folder.

## Names

A profile name is checked before any path is built from it. A name that would leave the profile
store is refused, so a profile cannot reach outside its folder. Use plain names such as `staff` or
`shift-2`.

## Consent answers

The answers to consent prompts are stored in `consent.txt` in the window's data folder. A named
profile keeps them. An ephemeral session drops them with its folder, so the user is asked again.
If the file cannot be opened, the answers are kept in memory for the run only.

## Wiping

- An ephemeral session is wiped when the app stops, through its guard. Wiping on window close, or
  after idle time, is not wired. WebView2 holds the files while a window is open (todo M6).
- `wipe_dir` overwrites file contents and then removes them. This is best effort. SSDs, wear
  levelling and journaling file systems can keep old copies, so do not rely on it for secrets
  that must not be recovered by forensic tools. Use full-disk encryption for that.

## Quotas

`usage(dir)` and `enforce_quota(dir, limit)` measure a profile and cap it. They run on the folder,
not on a live WebView2 profile yet.

## Export and import

`export_dir` (and `ProfileStore::export`) writes a profile folder to one file, encrypted with a
passphrase of at least 8 characters:

- The key comes from PBKDF2-HMAC-SHA256 with 600,000 iterations, a random salt and a random nonce.
- The payload is sealed with ChaCha20-Poly1305. The header is authenticated, so a changed salt or
  iteration count fails the check.
- Symbolic links are skipped.
- The file is written under a temporary name and renamed, so a failed export leaves no partial file.

Keep the passphrase apart from the file. A lost passphrase cannot be recovered.

## Status by platform

| Mapping | Windows | macOS | Linux |
|---|---|---|---|
| Default, named and ephemeral folders | Built | Not built | Not built |
| WebView2 user data folder, app-wide | Built | Not applicable | Not applicable |
| WebView2 user data folder, per window (`Window::profile`) | Built; isolation tested live on one machine | Not applicable | Not applicable |
| Data-store identifier (macOS 14+) | Not applicable | Not built | Not applicable |
| WebsiteDataManager base directories | Not applicable | Not applicable | Not built |
| `tpt.caps` reports the profile level | Not built | Not built | Not built |
| Conformance tests for isolation and no residue | Not done | Not done | Not done |

The file-level tests for the profile store and ephemeral folders pass on Windows. The todo keeps the
profile item open until the isolation and no-residue tests pass on all three systems (todo M6).
