// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! A minimal executor for `async` commands (spec §5.3). It runs one future to completion on the
//! calling thread and parks the thread while the future is pending. It uses only `std`, so no
//! async runtime is needed. A future that waits on another thread wakes this one through its
//! waker.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};

/// Wakes the thread that is blocked in [`block_on`].
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Runs `future` to completion on the current thread and returns its output.
///
/// The thread sleeps between polls, so a future that is ready at once costs nothing, and one
/// that waits on I/O or a channel is woken by its waker.
///
/// Support: Full.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            // A wake that happened before this park makes the park return at once, so the
            // loop cannot miss a wake.
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{ready, Future};
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::Poll;
    use std::time::Duration;

    /// Pends once, and wakes its waker from another thread after a short delay.
    struct WakeLater {
        started: bool,
    }

    impl Future for WakeLater {
        type Output = u32;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
            if self.started {
                return Poll::Ready(7);
            }
            self.started = true;
            let waker = cx.waker().clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(5));
                waker.wake();
            });
            Poll::Pending
        }
    }

    #[test]
    fn a_ready_future_returns_at_once() {
        assert_eq!(block_on(ready(3)), 3);
    }

    #[test]
    fn a_pending_future_is_woken_from_another_thread() {
        assert_eq!(block_on(WakeLater { started: false }), 7);
    }

    #[test]
    fn async_blocks_run_in_order() {
        let log = Mutex::new(Vec::new());
        let out = block_on(async {
            log.lock().unwrap().push("first");
            let n = WakeLater { started: false }.await;
            log.lock().unwrap().push("second");
            n + 1
        });
        assert_eq!(out, 8);
        assert_eq!(*log.lock().unwrap(), ["first", "second"]);
    }
}
