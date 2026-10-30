# tpt-shell-macros

The procedural macros for tpt-shell. They turn Rust functions into typed commands and embed an app's files in the binary.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell). The facade re-exports every macro here, so an app normally writes `use tpt_shell::command;` rather than depending on this crate.

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`).

## Macros

| Macro | What it does |
|---|---|
| `#[command]` | Registers a function as a typed command. Its argument and result types are read into the contract. |
| `commands![...]` | Groups several commands for registration. |
| `register_commands!` | Registers a group of commands with an app. |
| `embed_dir!("dir")` | Embeds every file under `dir`, relative to the calling crate's `Cargo.toml`. It expands to `&[(path, content_type, bytes)]`, with `/` separators and sorted paths, so the output is deterministic. |
| `include_manifest!` | Embeds and checks the app's `tpt-shell.toml` at compile time. |
| `#[derive(Contract)]` | Derives the contract for a type used in command arguments or results. |

## Commands

A command can declare the capability it needs. Its grant then checks the named argument:

```rust,ignore
// Illustrative. The attribute names the capability and the argument its scope checks.
#[command(capability = "fs.read", target(path = "file"))]
fn read_file(file: ScopedPath) -> Result<String, CommandError> {
    // ...
}
```

The capability name is checked against the app's grants. The `target` names the argument the grant's scope is checked against.

- A command with `capability = ...` registers behind its grant, so it cannot run through the unauthorised entry point.
- `target(path = "file")` names the argument the grant's scope checks. The target can be a path, a device, a URL or bytes.
- Async commands and job commands are supported. A job reports progress and can be cancelled. `#[command(job)]` is the form for jobs.
- The `ui` and `agent` flags mark a command as callable from the page's UI and from an agent.

Compile-fail tests cover misuse of the macros.

## Embedding files

Cargo rebuilds the crate when an embedded file changes. It does not notice a new file added to the directory. Touch a source file after adding one.

## Platform support

None. The macros only generate code, and the generated code runs on every platform.

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
