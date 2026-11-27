// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A dev run replays its last session after a reload (todo M8, dev auto-replay). The page counts its
//! clicks in the title. A click is made, a file in the dev folder changes, and the window reloads:
//! the saved click is performed again, so the title reads `count 1` after the page has reloaded.
//! The session file is written into the dev folder. Skipped unless `TPT_WEBVIEW2_LOADER` names
//! WebView2Loader.dll and a desktop is available. Needs the `agent` feature, since the page
//! replay uses the agent's page scripts.

#![cfg(all(windows, feature = "agent"))]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::backend::eval_window_json;
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

const INDEX: &str = r#"<!doctype html><title>starting</title>
<button id="go">Go</button>
<script src="app.js"></script>"#;

const APP_JS: &str = r#"
    var n = 0;
    document.title = 'ready';
    document.getElementById('go').addEventListener('click', function () {
      n += 1;
      document.title = 'count ' + n;
    });
"#;

#[test]
fn a_reload_replays_the_last_session_on_the_reloaded_page() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let base = std::env::temp_dir().join(format!("tpt-dev-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let dev = base.join("dev");
    std::fs::create_dir_all(&dev).expect("the dev folder");
    std::fs::write(dev.join("index.html"), INDEX).expect("index");
    std::fs::write(dev.join("app.js"), APP_JS).expect("app.js");
    // The app reads the dev folder when it starts, so the variable is set before the build.
    std::env::set_var("TPT_DEV_DIR", &dev);

    let running = App::new()
        .name("dev-replay-test")
        .loader(loader)
        .data_dir(base.join("data"))
        .window(Window::new("main").title("dev").size(640.0, 480.0))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let title = |running: &tpt_shell::Running| {
        running
            .backend()
            .document_title(id)
            .unwrap_or_default()
            .to_string()
    };

    assert!(
        pump_until(Duration::from_secs(30), || title(&running) == "ready"),
        "the page did not load: {}",
        title(&running)
    );
    eval_window_json(
        id,
        "document.getElementById('go').click(); 1",
        Duration::from_secs(10),
    )
    .expect("the click runs");
    assert!(
        pump_until(Duration::from_secs(10), || title(&running) == "count 1"),
        "the click was not counted: {}",
        title(&running)
    );

    // A change in the dev folder reloads the page. The saved click must then run again.
    std::fs::write(dev.join("marker.txt"), "changed").expect("the change");
    let mut seen_ready_after_reload = false;
    let replayed = pump_until(Duration::from_secs(60), || {
        let now = title(&running);
        if now == "ready" {
            seen_ready_after_reload = true;
        }
        seen_ready_after_reload && now == "count 1"
    });
    let saved = std::fs::read(dev.join(".tpt-dev").join("last.tptreplay")).ok();
    drop(running);
    let _ = std::fs::remove_dir_all(&base);

    assert!(seen_ready_after_reload, "the window did not reload");
    assert!(
        replayed,
        "the saved click was not replayed after the reload"
    );
    let saved = saved.expect("the session was saved in the dev folder");
    let replay = tpt_shell_replay::read_compressed(&saved, tpt_shell_replay::Mode::Lenient)
        .expect("the saved session reads back");
    let clicks = replay
        .events
        .iter()
        .filter(|e| e.kind == "input" && e.data["kind"] == "click")
        .count();
    assert_eq!(
        clicks, 1,
        "the saved session holds the one click, not the replay's"
    );
}
