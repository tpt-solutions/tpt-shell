// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A window whose page shows a stream of rendered frames on a `<canvas>`. A background
//! thread renders an animated image and sends each frame with `UiHandle::viewport_frame`.
//! The page draws it with `ImageData`. Windows only.
//!
//! This is the shell side of a 3D viewport. Any renderer that produces RGBA8 pixels can
//! feed it: an appfront `SceneDelegate` returns them from `pixels()`, and the host passes
//! them to `viewport_frame`. The example renders a gradient so it needs no 3D engine.
//!
//! usage: cargo run -p tpt-shell --example viewport -- <path to WebView2Loader.dll>

use std::thread;
use std::time::Duration;

use tpt_shell::{App, Window};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

// The page's script and CSS are separate assets. The shell's policy (`script-src 'self'`,
// `style-src 'self'`) blocks inline <script> and <style>, so inline code would not run.
const PAGE: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>viewport</title>
<link rel="stylesheet" href="viewport.css">
<canvas id="view"></canvas>
<div id="status">starting</div>
<script src="viewport.js"></script>
"#;

const STYLE: &str = r#"html, body { margin: 0; height: 100%; background: #101820; }
canvas { display: block; width: 100vw; height: 100vh; image-rendering: pixelated; }
#status { position: fixed; left: 8px; top: 8px; padding: 4px 8px; color: #fff;
  background: rgba(0, 0, 0, 0.6); font: 14px sans-serif; }
"#;

const SCRIPT: &str = r#"(function () {
  var canvas = document.getElementById('view');
  var ctx = canvas.getContext('2d');
  var status = document.getElementById('status');
  var frames = 0;
  window.tpt.on('viewport.frame', function (frame) {
    try {
      if (canvas.width !== frame.width || canvas.height !== frame.height) {
        canvas.width = frame.width;
        canvas.height = frame.height;
      }
      var binary = atob(frame.data);
      var pixels = new Uint8ClampedArray(binary.length);
      for (var i = 0; i < binary.length; i++) pixels[i] = binary.charCodeAt(i);
      ctx.putImageData(new ImageData(pixels, frame.width, frame.height), 0, 0);
      frames += 1;
      status.textContent = 'frames: ' + frames;
    } catch (err) {
      status.textContent = 'error: ' + err;
    }
  });
})();
"#;

/// One RGBA8 frame: a horizontal and vertical gradient whose blue channel moves with `t`.
fn gradient(t: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    let blue = (t * 4 % 256) as u8;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            out.push((x * 255 / (WIDTH - 1)) as u8);
            out.push((y * 255 / (HEIGHT - 1)) as u8);
            out.push(blue);
            out.push(255);
        }
    }
    out
}

fn main() -> tpt_shell::Result<()> {
    let loader = std::env::args()
        .nth(1)
        .ok_or(tpt_shell::Error::Unsupported {
            feature: "running without a WebView2Loader.dll path",
        })?;
    let running = App::new()
        .name("viewport")
        .loader(loader)
        .asset("index.html", "text/html", PAGE)
        .asset("viewport.css", "text/css", STYLE)
        .asset("viewport.js", "text/javascript", SCRIPT)
        .window(
            Window::new("main")
                .title("viewport")
                .size(640.0, 480.0)
                .page("index.html"),
        )
        .build()?;

    let ui = running.ui_handle();
    thread::spawn(move || {
        for t in 0.. {
            let frame = gradient(t);
            if let Err(err) = ui.viewport_frame("main", WIDTH, HEIGHT, &frame) {
                eprintln!("viewport: stopped sending frames: {err}");
                break;
            }
            thread::sleep(Duration::from_millis(33));
        }
    });

    running.run()
}
