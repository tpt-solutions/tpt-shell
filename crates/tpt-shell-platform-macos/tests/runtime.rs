// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Runtime checks of the macOS backend. They run only on a Mac with a GUI session, on the
//! process main thread (this test has its own `main`, `harness = false`). Anywhere else, and
//! when a check cannot run, it prints `SKIP` with the reason and still passes.

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIP: macOS runtime checks need macOS");
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
    use tpt_shell_core::backend::{
        Backdrop, PreferredScheme, WebviewBackend, WindowBackend, WindowSpec,
    };
    use tpt_shell_core::platform::{ClipboardBackend, Platform, ThemeBackend};
    use tpt_shell_core::{Error, LogicalSize};
    use tpt_shell_platform_macos::backend::MacBackend;

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    /// Runs the main run loop for `seconds`, so WebKit and AppKit make progress.
    fn pump(seconds: f64) {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < end {
            // SAFETY: the mode is a Foundation constant; the date is made for this call.
            unsafe {
                NSRunLoop::currentRunLoop().runMode_beforeDate(
                    NSDefaultRunLoopMode,
                    &NSDate::dateWithTimeIntervalSinceNow(0.02),
                );
            }
        }
    }

    fn spec() -> WindowSpec {
        WindowSpec {
            name: "runtime".into(),
            title: "runtime".into(),
            size: LogicalSize {
                width: 320.0,
                height: 240.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    pub fn run() {
        let Some(mut backend) = MacBackend::new() else {
            println!("SKIP: not on the main thread, so AppKit cannot run");
            return;
        };
        let mtm = objc2::MainThreadMarker::new().expect("main thread, checked above");
        let app = NSApplication::sharedApplication(mtm);
        if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
            println!("SKIP: no GUI session (the activation policy could not be set)");
            return;
        }

        window_features(&mut backend);
        snapshot_is_png(&mut backend);
        clipboard_round_trip(&mut backend);
        theme_reads(&backend);
        keystore_round_trip(&mut backend);
        single_instance_needs_a_bundle();
        tray_shows_and_goes();
        println!("macOS runtime checks done");
    }

    fn window_features(backend: &mut MacBackend) {
        let id = backend.create(&spec()).expect("create");
        backend.show(id).expect("show");
        backend.set_frameless(id, true).expect("frameless on");
        backend.set_frameless(id, false).expect("frameless off");
        backend.set_always_on_top(id, true).expect("top on");
        backend.set_always_on_top(id, false).expect("top off");
        backend.set_transparent(id, true).expect("transparent on");
        backend
            .set_backdrop(id, Backdrop::Mica)
            .expect("backdrop on");
        backend
            .set_backdrop(id, Backdrop::Acrylic)
            .expect("backdrop change");
        backend
            .set_backdrop(id, Backdrop::None)
            .expect("backdrop off");
        backend.set_transparent(id, false).expect("transparent off");
        backend
            .set_preferred_color_scheme(id, PreferredScheme::Dark)
            .expect("scheme dark");
        backend
            .set_preferred_color_scheme(id, PreferredScheme::System)
            .expect("scheme system");
        backend.close(id).expect("close");
        println!("ok: window features");
    }

    fn snapshot_is_png(backend: &mut MacBackend) {
        let id = backend.create(&spec()).expect("create");
        backend.show(id).expect("show");
        backend
            .load_html(id, "<body style='background:#fff'>snapshot</body>")
            .expect("load");
        pump(1.0);
        match backend.snapshot(id) {
            Ok(png) => {
                assert!(png.starts_with(&PNG_SIGNATURE), "snapshot is not a PNG");
                println!("ok: snapshot is {} bytes of PNG", png.len());
            }
            // A headless runner may have no compositor to draw from.
            Err(e) => println!("SKIP: snapshot unavailable here: {e}"),
        }
        backend.close(id).expect("close");
    }

    fn clipboard_round_trip(backend: &mut MacBackend) {
        let text = "tpt-shell clipboard check";
        match backend.set_clipboard_text(text) {
            Ok(()) => {
                assert_eq!(backend.clipboard_text().unwrap().as_deref(), Some(text));
                println!("ok: clipboard round trip");
            }
            Err(e) => println!("SKIP: pasteboard unavailable: {e}"),
        }
    }

    fn theme_reads(backend: &MacBackend) {
        let scheme = backend.color_scheme().expect("color scheme");
        let seen = Rc::new(Cell::new(false));
        let flag = seen.clone();
        let observer = tpt_shell_platform_macos::theme::observe_appearance(move |_| flag.set(true));
        let system = tpt_shell_platform_macos::theme::system_scheme();
        drop(observer);
        println!("ok: theme {scheme:?}, system {system:?}, observer added and removed");
    }

    fn keystore_round_trip(backend: &mut MacBackend) {
        let name = "tpt-shell-runtime-check";
        match backend.keystore_set(name, b"secret one") {
            Ok(()) => {
                assert_eq!(
                    backend.keystore_get(name).unwrap().as_deref(),
                    Some(&b"secret one"[..])
                );
                backend.keystore_set(name, b"secret two").unwrap();
                assert_eq!(
                    backend.keystore_get(name).unwrap().as_deref(),
                    Some(&b"secret two"[..])
                );
                assert!(backend.keystore_delete(name).unwrap());
                assert_eq!(backend.keystore_get(name).unwrap(), None);
                assert!(!backend.keystore_delete(name).unwrap());
                println!("ok: keystore round trip");
            }
            // A runner without an unlocked login keychain refuses writes.
            Err(e) => println!("SKIP: keychain unavailable: {e}"),
        }
    }

    fn single_instance_needs_a_bundle() {
        // A test binary is not an app bundle, so there is no bundle identifier to key on.
        match tpt_shell_platform_macos::single_instance::acquire(&[], |_| {}) {
            Err(Error::Unsupported { .. }) => println!("ok: single instance refuses a bare binary"),
            Ok(_) => println!("SKIP: this process has a bundle identifier"),
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    fn tray_shows_and_goes() {
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        match tpt_shell_platform_macos::tray::Tray::new(
            mtm,
            "T",
            "tpt-shell",
            None,
            &["Open", tpt_shell_platform_macos::tray::SEPARATOR, "Quit"],
            Box::new(|_| {}),
        ) {
            Ok(tray) => {
                tray.set_visible(true);
                pump(0.2);
                drop(tray);
                println!("ok: tray added and removed");
            }
            Err(e) => println!("SKIP: no menu bar here: {e}"),
        }
    }

    // Referenced so an unused-import lint cannot hide a removed dialog path: dialogs are modal
    // and need a person, so they are type-checked only.
    #[allow(dead_code)]
    fn dialogs_compile(backend: &mut MacBackend) {
        let _ = backend.message_box(None, &Default::default());
        let _ = backend.open_files(None, &Default::default());
    }
}
