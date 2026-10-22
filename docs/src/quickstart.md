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

`tpt-shell` opens windows through `WebView2Loader.dll`, which ships in the WebView2 SDK
package. Take the x64 copy from `build/native/x64/` in that package, and pass its path to the
example:

```sh
cargo run -p tpt-shell --example hello -- path\to\WebView2Loader.dll
```

A window shows a page that says "hello from tpt-shell". Close it to stop. A second launch hands
its arguments to the first window and exits.

## 3. Create a project

The folder must be new or empty:

```sh
tpt-shell new vanilla my-app
```

To make a project that opens a window, use `tpt-shell new rust my-app` instead, then
`tpt-shell run my-app`. Copy `WebView2Loader.dll` into `my-app/target/debug/` first, because the
program looks for it next to itself.

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
change within about half a second. Use `--port <port>` if 8080 is taken.

A plain browser tab has no `window.tpt`, the page runtime that only a shell window provides, so
commands will not run there. The preview shows the layout and nothing more.

## Not yet

The `vanilla` and `typescript` projects do not open a window by themselves. Use the `rust`
starter (`tpt-shell new rust my-app`), which builds the window from the project's files. Its
window needs `WebView2Loader.dll` beside the built program, or its path as the first argument;
the loader still has to be copied from the WebView2 SDK by hand. The example's source is
`crates/tpt-shell/examples/hello.rs`.
