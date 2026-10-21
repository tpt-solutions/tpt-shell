# Five-minute quickstart

This page covers what works today: scaffolding a project, checking its manifest, and serving its
files for a preview. Opening the app in a native window needs a Rust program that builds a
`tpt_shell::App`. There is no example of that yet, so this page stops before it.

The crates are not published yet, so install the CLI from a checkout of the repository:

```sh
cargo install --path crates/tpt-shell-cli
```

Create a project. The folder must be new or empty:

```sh
tpt-shell new vanilla my-app
```

Check its manifest:

```sh
tpt-shell manifest check my-app/tpt-shell.toml
# my-app/tpt-shell.toml: ok (1 window(s), 0 capability grant(s))
```

Serve the app's files on the loopback address:

```sh
tpt-shell dev my-app
# serving C:\...\my-app at http://127.0.0.1:8080/ (Ctrl+C to stop)
```

Open <http://127.0.0.1:8080/> in a browser. Edit a file in `my-app` and the terminal reports the
change within about half a second. Use `--port <port>` if 8080 is taken.

A plain browser tab has no `window.tpt`, the page runtime that only a shell window provides, so
commands will not run there. The preview shows the page layout and nothing more.
