// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The licence page is served from the app origin at `__tpt/licenses` in a real window. The page
//! fetches it the way any app page would and reports what came back. Skipped unless
//! `TPT_WEBVIEW2_LOADER` names WebView2Loader.dll and a desktop is available.

#![cfg(windows)]

use std::time::Duration;

use tpt_shell::{App, Window};
use tpt_shell_platform_windows::window::{pump_until, runtime_test_loader};

/// The start page. Its script is a separate asset, because the page policy blocks inline script.
const INDEX: &str = r#"<!doctype html><title>start</title><script src="app.js"></script>"#;

/// Fetches the licence page. The title becomes `licences-ok` when it is HTML from the server
/// with a licence title in it. Anything else is reported in the title, so a failure says why.
const APP_JS: &str = r#"
    fetch('__tpt/licenses').then(function (response) {
      var type = response.headers.get('content-type') || '';
      return response.text().then(function (text) {
        var good = response.ok && type.indexOf('text/html') === 0 && text.indexOf('<title>Licen') !== -1;
        document.title = good ? 'licences-ok' : 'licences-bad:' + response.status + ':' + type;
      });
    }).catch(function (error) {
      document.title = 'licences-error:' + error;
    });
"#;

#[test]
fn the_licence_page_loads_from_the_app_origin() {
    let Some((loader, _serial)) = runtime_test_loader() else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("tpt-app-licences-{}", std::process::id()));

    // No licence page is supplied: the facade serves its own at `__tpt/licenses`.
    let running = App::new()
        .name("licences-test")
        .loader(loader)
        .data_dir(&dir)
        .asset("index.html", "text/html", INDEX)
        .asset("app.js", "text/javascript", APP_JS)
        .window(Window::new("main").title("licences-test"))
        .build()
        .expect("the app should start");
    let id = running.window("main").expect("the main window");
    let mut title = String::new();
    let got = pump_until(Duration::from_secs(30), || {
        title = running.backend().document_title(id).unwrap_or_default();
        title.starts_with("licences-")
    });
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        got,
        "the page never reported on the licence page; title was {title:?}"
    );
    assert_eq!(
        title, "licences-ok",
        "the licence page did not load as expected"
    );
}
