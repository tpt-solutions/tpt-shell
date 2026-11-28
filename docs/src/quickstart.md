# Five-minute quickstart

This page takes you from a checkout to a window and a preview. Windows only for now: the
window needs the WebView2 runtime, and the other platforms are not built yet.

You need the Rust toolchain (1.90 or later), and the Evergreen WebView2 runtime, which is
installed on current Windows 10 and 11 machines.

## 1. Install the CLI

The crates are not published yet, so install the CLI from a checkout of the repository:

```sh
cargo install --path crates/tpt-shell-cli
```

## 2. Open a window

`tpt-shell` opens windows through `WebView2Loader.dll`. The repository vendors the x64 copy in
`redist/win-x64/`, so pass that path to the example:

```sh
cargo run -p tpt-shell --example hello -- redist\win-x64\WebView2Loader.dll
```

A window shows a page that says "hello from tpt-shell". Close it to stop. A second launch hands
its arguments to the first window and exits.

## 3. Create a project

The folder must be new or empty:

```sh
tpt-shell new vanilla my-app
```

To make a project that opens a window, use `tpt-shell new rust my-app` instead, then
`tpt-shell run my-app`. The run builds the project and copies the vendored `WebView2Loader.dll`
into `my-app/target/debug/`, beside the program, so there is no manual copy step. The copy happens
only when the CLI was installed from this checkout, which has the vendored file.

Check its manifest:

```sh
tpt-shell manifest check my-app/tpt-shell.toml
# my-app/tpt-shell.toml: ok (1 window(s), 0 capability grant(s))
```

## 4. Preview the project's files

Serve them on the loopback address:

```sh
tpt-shell dev my-app
# serving C:\...\my-app at http://127.0.0.1:8080/ (Ctrl+C to stop)
```

Open <http://127.0.0.1:8080/> in a browser. Edit a file in `my-app` and the terminal reports the
change within about half a second, and the browser tab reloads. Use `--port <port>` if 8080 is
taken.

A plain browser tab has no real `window.tpt`, the page runtime that only a shell window provides.
`dev` adds a mock in its place, so the page loads and its commands run. A command has no answer
until the page sets one, for example `window.tpt.mock.respond('greet', args => ...)`. A command
with no answer rejects with `MOCK_NO_RESPONDER`. The mock does nothing inside a shell window.

The shell window itself does not reload on change. It loads the app's own files, not the dev
server.

## Not yet

The `vanilla` and `typescript` projects do not open a window by themselves. Use the `rust`
starter (`tpt-shell new rust my-app`), which builds the window from the project's files. `tpt-shell
run` places the vendored loader beside the built program. A program built some other way needs
`WebView2Loader.dll` beside it, or its path set with `App::loader`. The example's source is
`crates/tpt-shell/examples/hello.rs`.
