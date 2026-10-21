# Cookbook

Each recipe is short and does one thing. Recipes marked `compile-check` are compiled by
`tools/doc-samples.py` against the facade crate, so they match the API in this checkout. The
others show a command, a file or a larger pattern, and say which crate they need.

Recipes assume the Windows backend, the WebView2 loader DLL path passed in, and the facade crate
`tpt-shell`. Nothing here runs on macOS or Linux yet.

## Windows and pages

### 1. One window with one page

```rust,compile-check
use tpt_shell::{App, Window};

const PAGE: &str = "<!doctype html><title>hi</title><h1>hi</h1>";

fn main() -> tpt_shell::Result<()> {
    App::new()
        .name("hi")
        .loader("WebView2Loader.dll")
        .asset("index.html", "text/html", PAGE)
        .window(Window::new("main").title("Hi"))
        .run()
}
```

The loader path must point at the `WebView2Loader.dll` from the WebView2 SDK package.

### 2. Set the window size

```rust,compile-check
use tpt_shell::Window;

fn main() {
    let _main = Window::new("main").title("Till").size(1024.0, 768.0);
}
```

Sizes are logical pixels. The window name is its identity in grants, so keep it stable.

### 3. Open a second page in the same window

```rust,compile-check
use tpt_shell::Window;

fn main() {
    let _settings = Window::new("settings").page("settings.html");
}
```

The page path is relative to the app origin.

### 4. Serve several files

```rust,compile-check
use tpt_shell::App;

fn main() {
    let _app = App::new().assets(&[
        ("index.html", "text/html", &b"<!doctype html><script src=\"app.js\"></script>"[..]),
        ("app.js", "text/javascript", &b"console.log('ready');"[..]),
        ("app.css", "text/css", &b"body { margin: 0 }"[..]),
    ]);
}
```

Pages cannot use inline script or style, so put them in files. Add each file with `asset`, or pass a
list to `assets`.

### 5. Embed a whole folder

```rust
// Plain Rust, not compiled here: the folder must exist next to the calling crate's Cargo.toml.
use tpt_shell::{embed_dir, App};

fn app() -> App {
    App::new().assets(embed_dir!("dist"))
}
```

`embed_dir!` returns the same list `assets` takes. Cargo rebuilds only when an embedded file changes,
so touch a source file after you add a new file to the folder.

## Launch behaviour

### 6. Only one copy runs

```rust,compile-check
use tpt_shell::App;

fn main() {
    let _app = App::new()
        .name("till")
        .single_instance("till-main", |args: Vec<String>| {
            eprintln!("second launch with {args:?}");
        });
}
```

A later launch passes its arguments to the first copy and exits. The first copy's own arguments
are not passed on.

### 7. Register a custom link scheme

```rust,compile-check
use tpt_shell::App;

fn main() {
    let _app = App::new().name("till").deep_link("till");
}
```

Windows registers the scheme for the current user when the app starts. A `till://...` link then
starts the app, or reaches the running copy through `single_instance`.

### 8. Remember window placement

```rust,compile-check
use tpt_shell::App;

fn main() {
    let _app = App::new().name("till").remember_window_state("window-state.json");
}
```

The file is read at start and written when the event loop ends. A missing or broken file means the
windows open in their default places.

### 9. Use a tray icon with a menu

```rust,compile-check
use tpt_shell::{App, MenuItem, TRAY_ACTIVATE};

fn main() {
    let _app = App::new().name("till").tray(
        "Till is running",
        vec![
            MenuItem { id: 1, label: "Open".into() },
            MenuItem { id: 2, label: "Quit".into() },
        ],
        |id: u32| {
            if id == TRAY_ACTIVATE {
                eprintln!("left click");
            } else if id == 2 {
                eprintln!("quit picked");
            }
        },
    );
}
```

A left click reports `TRAY_ACTIVATE`. A menu pick reports that item's id. The menu is shown on
Windows only.

## Commands and capabilities

### 10. A command with a capability and a grant

```rust,compile-check
use tpt_shell::{App, CommandError, Grant, Scope, Value};

fn greet(args: &Value) -> Result<Value, CommandError> {
    let name = args["name"].as_str().unwrap_or("you");
    Ok(Value::String(format!("hello, {name}")))
}

fn main() {
    let _app = App::new()
        .name("greet")
        .command("greet", "app.greet", |_args: &Value| tpt_shell::Target::None, greet)
        .grant(Grant {
            capability: "app.greet".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Unrestricted,
            consent: false,
        });
}
```

A page calls it with `window.tpt.invoke("greet", { name: "Ada" })`. Without the grant, the call fails
with `CAP_NO_GRANT`.

### 11. A typed command from a function signature

```rust,compile-check
use tpt_shell::{command, register_commands, App, CommandError};

#[command(capability = "app.greet")]
fn greet(name: String) -> Result<String, CommandError> {
    Ok(format!("hello, {name}"))
}

fn main() {
    let _app = App::new()
        .name("greet")
        .register(|router| register_commands!(router, greet));
}
```

The macro builds the argument schema from the types. Unknown fields and wrong types are refused with
`IPC_INVALID_ARGS` before `greet` runs. Commands with no target use only unrestricted grants. For a
command that acts on a path, use the hand-written form in recipe 12.

### 12. A command that reads a path, with a scope

```rust,compile-check
use tpt_shell::{App, CommandError, Grant, Scope, Target, Value};

fn read_note(args: &Value) -> Result<Value, CommandError> {
    let file = args["file"].as_str().unwrap_or_default();
    Ok(Value::String(format!("would read {file}")))
}

fn main() {
    let _app = App::new()
        .name("notes")
        .command(
            "read_note",
            "fs.read",
            |args: &Value| Target::Path(args["file"].as_str().unwrap_or_default().to_string()),
            read_note,
        )
        .grant(Grant {
            capability: "fs.read".into(),
            window: "main".into(),
            origin: "https://tpt-app.localhost".into(),
            scope: Scope::Paths(vec!["C:/Notes".into()]),
            consent: false,
        });
}
```

The target function says what the call acts on. The grant allows only files under `C:/Notes`. A
request for `C:/Secrets/key` fails with `CAP_OUT_OF_SCOPE`. A request with no `file` fails with
`CAP_BAD_REQUEST`.

### 13. Ask the user before a grant is used

```rust,compile-check
use tpt_shell::{Grant, Scope};

fn main() {
    let _export = Grant {
        capability: "fs.write".into(),
        window: "main".into(),
        origin: "https://tpt-app.localhost".into(),
        scope: Scope::Paths(vec!["C:/Exports".into()]),
        consent: true,
    };
}
```

The first use asks the user in a native dialog. The answer is kept per window and capability. The
default answer is No.

### 14. Return a structured error from a command

```rust,compile-check
use tpt_shell::{CommandError, Value};

fn check_amount(args: &Value) -> Result<Value, CommandError> {
    let amount = args["amount"].as_i64().unwrap_or(0);
    if amount <= 0 {
        return Err(
            CommandError::new("APP_BAD_AMOUNT", "the amount must be above 0")
                .with_details(Value::from(amount)),
        );
    }
    Ok(Value::Bool(true))
}

fn main() {
    let _ = check_amount;
}
```

Use your own codes with the `APP_` prefix. Keep each code stable, because pages may test for it. Add a
row to your app's error list with an explanation and a fix.

### 15. Send an event to a window

```rust
// Plain Rust, not compiled here: `Running` exists on Windows only.
fn notify_ready(running: &mut tpt_shell::Running) -> tpt_shell::Result<()> {
    running.emit("main", "scan:done", serde_json::json!({ "count": 3 }))
}
```

The page listens with `window.tpt.on("scan:done", (data) => { ... })`. `emit` takes the window name.
Use `serde_json` in your own crate to build the data.

## Audit, state and profiles

### 16. Audit every command call

```rust,compile-check
use std::path::Path;
use tpt_shell::App;

fn main() -> tpt_shell::Result<()> {
    let _app = App::new()
        .name("till")
        .manifest_audit(Path::new("tpt-shell.toml"), "audit.jsonl")?;
    Ok(())
}
```

The `[audit]` section of the manifest chooses what is kept. By default only hashes are kept. See the
[Audit guide](guides/audit.md).

### 17. Check an audit log after an incident

```sh
tpt-shell audit verify audit.jsonl --key audit-public.key
tpt-shell audit export audit.jsonl --format csv > audit.csv
```

Verify first. Export also verifies, and refuses a log whose chain is broken.

### 18. Save and restore app state every 30 seconds

```rust,compile-check
use std::time::Duration;
use tpt_shell::{App, Value};

fn main() {
    let _app = App::new().name("till").snapshot_state(
        "state.json",
        Duration::from_secs(30),
        || Value::from("draft text"),
        |saved: Value| eprintln!("restored {saved}"),
    );
}
```

`capture` runs on the UI thread, so keep it quick. `restore` runs once before the windows show. Do not
store secrets here.

### 19. Use a temporary session that leaves nothing behind

```rust,compile-check
use tpt_shell::{App, ProfileKind};

fn main() {
    let _app = App::new().name("kiosk").profile(ProfileKind::Ephemeral);
}
```

The session folder is deleted when the app stops. Consent answers go with it, so the user is asked
again next time.

### 20. Keep a named profile for one staff member

```rust,compile-check
use tpt_shell::{App, ProfileKind};

fn main() {
    let _app = App::new().name("till").profile(ProfileKind::Named("shift-2".into()));
}
```

A named profile lasts until it is deleted. Use a plain name. The shell refuses names that could reach
outside the profile store.

### 21. Choose the data folder directly

```rust,compile-check
use tpt_shell::App;

fn main() {
    let _app = App::new().name("till").data_dir("D:/till-data");
}
```

An explicit folder wins over the profile setting.

## Kiosk and devices

### 22. Start a kiosk from a signed config

```rust,compile-check
use std::path::Path;
use tpt_shell::{App, Window};

fn main() -> tpt_shell::Result<()> {
    let _app = App::new()
        .name("kiosk")
        .loader("WebView2Loader.dll")
        .asset("index.html", "text/html", "<!doctype html><title>kiosk</title>")
        .window(Window::new("main"))
        .kiosk_config(
            Path::new("kiosk.json"),
            Path::new("kiosk.json.sig"),
            "0000000000000000000000000000000000000000000000000000000000000000",
        )?;
    Ok(())
}
```

The public key shown is a placeholder: 32 bytes as hex. Use your real key. The config is refused when
its signature does not match. See the [Kiosk guide](guides/kiosk.md).

### 23. Print a receipt to a virtual printer in a test

```rust,compile-check
use std::str::FromStr;
use tpt_shell::devices::escpos::{Align, Receipt};
use tpt_shell::devices::hub::DeviceHub;
use tpt_shell::devices::id::{DeviceId, DeviceScope};
use tpt_shell::devices::hub::Event;
use tpt_shell::devices::virtual_devices;

fn print_test_receipt() -> Result<(), String> {
    let devices = virtual_devices::from_spec("printer@usb:0416:5011")?;
    let mut hub = DeviceHub::new(devices, |_event: &Event| {});
    let id = DeviceId::from_str("usb:0416:5011").map_err(|e| e.0)?;
    let scope = DeviceScope::Usb { vendor: Some(0x0416), product: Some(0x5011) };
    let mut printer = hub.open(&id, &scope).map_err(|e| format!("{e:?}"))?;

    let mut receipt = Receipt::new();
    receipt
        .align(Align::Center)
        .bold(true)
        .line("Test shop")
        .map_err(|e| format!("{e:?}"))?;
    receipt.feed(2).cut(true);
    printer.write(&receipt.into_bytes()).map_err(|e| format!("{e:?}"))?;
    Ok(())
}

fn main() {
    let _ = print_test_receipt;
}
```

The virtual printer keeps the bytes it receives, so a test can compare them with the expected output.
Text must be ASCII.

### 24. Lay out a standard receipt

```rust,compile-check
use tpt_shell::devices::jobs;

fn main() {
    let bytes = jobs::receipt("Coffee shop", &[("Latte", "3.50"), ("Muffin", "2.00")], "5.50");
    let _ = bytes;
}
```

`jobs::receipt` returns the ESC/POS bytes, or an error when the text is not ASCII. Sending the bytes
needs a device. See the [Devices guide](guides/devices.md) for what is built on each platform.

## Checks and tools

### 25. Check a manifest before you build

```sh
tpt-shell manifest check tpt-shell.toml
```

The command prints `ok` with the window and grant counts, or the problems with their `TS####` codes.
Exit code 1 means the file has problems. Look up any code with `tpt-shell explain TS0003`.

### 26. Preview the project's files in a browser

```sh
tpt-shell dev my-app --port 8080
```

The preview serves the files on 127.0.0.1 and reports changes. A plain browser has no `window.tpt`, so
commands do not run in it.

### 27. Build a release binary

```sh
tpt-shell build my-app
```

The command runs `cargo build --release --locked` in the directory, which must hold a `Cargo.toml`.

### 28. Write the compliance files for a release

```sh
tpt-shell compliance out/compliance
```

See the [Compliance guide](guides/compliance.md) for the three files it writes.

### 29. Check the audit log of a release on another machine

```sh
tpt-shell audit view audit.jsonl
```

`view` prints the records. Use `verify` to check the chain first.

### 30. Replay a recording in code

Replays are not built as a command yet. The reader and divergence check are in the
`tpt-shell-replay` crate. See the [Replay format](replay-format.md).
