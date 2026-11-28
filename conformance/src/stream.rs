// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The streams check, shared by every backend. The host sends a stream of chunks to a page through
//! the platform's message channel (`post_message` out, the message handler in). Each chunk costs
//! one credit (`tpt_shell_ipc::stream::Sender`), and the page returns a credit for each chunk it
//! consumes. The check passes when:
//! - every chunk arrives once, in order, with the right payload, while the page stalls now and then;
//! - the host never has more chunks in flight than the credit it was given;
//! - the host used all of its credit at some point, so the back-pressure path ran.
//!
//! The host side is `StreamHost`. It needs only `WebviewBackend::post_message` and a message
//! handler, so it runs unchanged on each platform.

use tpt_shell_ipc::stream::Sender;

/// Chunks in the stream.
pub const CHUNKS: usize = 32;
/// Payload bytes per chunk.
pub const CHUNK_BYTES: usize = 16 * 1024;
/// Credit the host starts with: the most chunks that may be in flight.
pub const CREDIT: u32 = 4;

/// The page script for the check. It posts `ready` when it starts, consumes each chunk, returns a
/// credit with `credit:1`, stalls for 20 ms after every eighth chunk, and at the end posts
/// `result:ok` or `result:bad` (and sets the title to `done:<count>:ok` or `done:<count>:bad`).
pub const PAGE_JS: &str = r#"// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
var TOTAL = 32, SIZE = 16384, next = 0, bad = 0;
function ack() { window.chrome.webview.postMessage("credit:1"); }
window.chrome.webview.addEventListener("message", function (e) {
  var data = e.data, bar = data.indexOf("|");
  var index = parseInt(data.slice(0, bar), 10), payload = data.slice(bar + 1);
  var letter = String.fromCharCode(97 + (index % 26));
  if (index !== next || payload.length !== SIZE || payload.split(letter).join("") !== "") { bad++; }
  next++;
  if (next % 8 === 0 && next < TOTAL) {
    setTimeout(ack, 20);
  } else {
    ack();
  }
  if (next === TOTAL) {
    document.title = "done:" + next + ":" + (bad === 0 ? "ok" : "bad");
    window.chrome.webview.postMessage("result:" + (bad === 0 ? "ok" : "bad"));
  }
});
window.chrome.webview.postMessage("ready");
"#;

/// The host side of the stream: the sender with credit, and what was posted.
pub struct StreamHost {
    sender: Sender,
    ready: bool,
    next: usize,
    acked: usize,
    max_in_flight: usize,
    result: Option<String>,
}

impl Default for StreamHost {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamHost {
    pub fn new() -> Self {
        StreamHost {
            sender: Sender::new(CREDIT, CHUNK_BYTES + 16),
            ready: false,
            next: 0,
            acked: 0,
            max_in_flight: 0,
            result: None,
        }
    }

    /// Handles one message from the page. The host answers nothing: the chunks go out through
    /// `next_posts`.
    pub fn on_message(&mut self, message: &str) {
        match message {
            "ready" => self.ready = true,
            "credit:1" => {
                self.acked += 1;
                self.sender.grant(1);
            }
            "result:ok" => self.result = Some("ok".into()),
            "result:bad" => self.result = Some("bad".into()),
            _ => {}
        }
    }

    /// The messages to post to the page now: as many chunks as the credit allows. Call this after
    /// each message, and whenever the page is ready.
    pub fn next_posts(&mut self) -> Vec<String> {
        if self.ready {
            while self.next < CHUNKS && self.sender.credit() > 0 {
                let chunk = format!("{}|{}", self.next, payload(self.next));
                self.sender
                    .send(chunk.into_bytes())
                    .expect("credit was checked, and the chunk is within the limit");
                self.next += 1;
            }
        }
        let mut out = Vec::new();
        while let Some(bytes) = self.sender.take() {
            out.push(String::from_utf8(bytes).expect("chunks are ASCII"));
        }
        self.max_in_flight = self.max_in_flight.max(self.next - self.acked);
        out
    }

    /// True once the page has reported its result and every chunk is acknowledged. The page's
    /// result can arrive before its last stalled acknowledgement, so both are waited for.
    pub fn complete(&self) -> bool {
        self.result.is_some() && self.acked == CHUNKS
    }

    /// The largest number of chunks in flight at once.
    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    pub fn acked(&self) -> usize {
        self.acked
    }
}

/// The payload of chunk `index`: `CHUNK_BYTES` copies of one letter that depends on the index.
pub fn payload(index: usize) -> String {
    let letter = char::from(b'a' + (index % 26) as u8);
    letter.to_string().repeat(CHUNK_BYTES)
}

/// The check's verdict from the page's result and the host's counts.
pub fn verdict(host: &StreamHost) -> Result<String, String> {
    match host.result.as_deref() {
        Some("ok") => {}
        Some(other) => {
            return Err(format!(
                "the page saw chunks out of order or with bad payloads ({other})"
            ))
        }
        None => return Err("the page did not report a result".into()),
    }
    if host.acked() != CHUNKS {
        return Err(format!(
            "the page acknowledged {} of {CHUNKS} chunks",
            host.acked()
        ));
    }
    if host.max_in_flight() > CREDIT as usize {
        return Err(format!(
            "{} chunks were in flight, more than the credit of {CREDIT}",
            host.max_in_flight()
        ));
    }
    if host.max_in_flight() < CREDIT as usize {
        return Err(format!(
            "the host never used its whole credit (at most {} in flight), so back-pressure was not exercised",
            host.max_in_flight()
        ));
    }
    Ok(format!(
        "{CHUNKS} chunks of {} KiB arrived once and in order while the page stalled; at most {} in flight against a credit of {CREDIT}",
        CHUNK_BYTES / 1024,
        host.max_in_flight()
    ))
}
