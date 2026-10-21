// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Minimal async executor and worker pool, std only (spec §5.6). No external runtime.
//!
//! [`block_on`] runs one future on the calling thread. [`Executor`] runs many futures and
//! blocking jobs on a fixed set of worker threads. A future is polled on whichever worker
//! wakes it, one poll at a time, so it never runs on two threads at once.

use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle};

use crate::error::{Error, Result};

type Job = Box<dyn FnOnce() + Send>;
type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

enum Work {
    Run(Job),
    Poll(Arc<Task>),
    Stop,
}

/// A spawned future. The future stays in the mutex during a poll, so a wake that arrives
/// mid-poll queues another poll that runs after this one finishes, and no wake is lost.
struct Task {
    future: Mutex<Option<BoxFuture>>,
    queue: Sender<Work>,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        let queue = self.queue.clone();
        // A failed send means the executor is gone and nothing is left to poll.
        let _ = queue.send(Work::Poll(self));
    }
}

/// Fixed-size worker pool that runs jobs and polls spawned futures.
pub struct Executor {
    queue: Sender<Work>,
    workers: Vec<JoinHandle<()>>,
}

impl Executor {
    /// Starts `threads` workers. Zero is raised to one.
    pub fn new(threads: usize) -> Self {
        let (queue, rx) = channel::<Work>();
        let rx = Arc::new(Mutex::new(rx));
        let count = threads.max(1);
        let workers = (0..count)
            .map(|_| {
                let rx = rx.clone();
                thread::spawn(move || worker_loop(&rx))
            })
            .collect();
        Executor { queue, workers }
    }

    /// Runs `job` on a worker. A panic in the job is caught, so the worker keeps running.
    pub fn execute(&self, job: impl FnOnce() + Send + 'static) -> Result<()> {
        self.send(Work::Run(Box::new(job)))
    }

    /// Spawns `future` onto the pool. It is polled until it completes.
    pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) -> Result<()> {
        let task = Arc::new(Task {
            future: Mutex::new(Some(Box::pin(future))),
            queue: self.queue.clone(),
        });
        self.send(Work::Poll(task))
    }

    fn send(&self, work: Work) -> Result<()> {
        self.queue
            .send(work)
            .map_err(|_| Error::Backend("executor has shut down".into()))
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // Each worker takes one Stop and exits. Jobs queued before the Stops still run.
        for _ in &self.workers {
            let _ = self.queue.send(Work::Stop);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(rx: &Mutex<Receiver<Work>>) {
    loop {
        let work = match rx.lock() {
            Ok(rx) => rx.recv(),
            Err(_) => return,
        };
        match work {
            Ok(Work::Run(job)) => {
                let _ = catch_unwind(AssertUnwindSafe(job));
            }
            Ok(Work::Poll(task)) => poll_task(task),
            Ok(Work::Stop) | Err(_) => return,
        }
    }
}

fn poll_task(task: Arc<Task>) {
    let mut slot = match task.future.lock() {
        Ok(slot) => slot,
        Err(_) => return,
    };
    let Some(future) = slot.as_mut() else {
        // Already finished: a late wake for a completed future.
        return;
    };
    let waker = Waker::from(task.clone());
    let mut cx = Context::from_waker(&waker);
    let finished = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
    match finished {
        Ok(Poll::Ready(())) | Err(_) => *slot = None,
        Ok(Poll::Pending) => {}
    }
}

/// Runs `future` to completion on the calling thread.
pub fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    #[test]
    fn block_on_waits_for_a_wake_from_another_thread() {
        let mut polls = 0;
        let value = block_on(poll_fn(|cx| {
            polls += 1;
            if polls == 1 {
                let waker = cx.waker().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(5));
                    waker.wake();
                });
                Poll::Pending
            } else {
                Poll::Ready(polls)
            }
        }));
        assert_eq!(value, 2);
    }

    #[test]
    fn jobs_run_on_workers() {
        let exec = Executor::new(2);
        let (tx, rx) = sync_channel(4);
        for i in 0..4 {
            let tx = tx.clone();
            exec.execute(move || tx.send(i * 10).unwrap()).unwrap();
        }
        let mut got: Vec<i32> = (0..4).map(|_| rx.recv().unwrap()).collect();
        got.sort();
        assert_eq!(got, vec![0, 10, 20, 30]);
    }

    #[test]
    fn a_panicking_job_does_not_stop_the_worker() {
        let exec = Executor::new(1);
        exec.execute(|| panic!("job failed on purpose")).unwrap();
        let (tx, rx) = sync_channel(1);
        exec.execute(move || tx.send(()).unwrap()).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn spawned_future_completes_across_wakes() {
        let exec = Executor::new(2);
        let (tx, rx) = sync_channel(1);
        let mut polls = 0;
        exec.spawn(poll_fn(move |cx| {
            polls += 1;
            if polls < 3 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                tx.send(polls).unwrap();
                Poll::Ready(())
            }
        }))
        .unwrap();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 3);
    }

    #[test]
    fn drop_runs_queued_jobs_before_returning() {
        let (tx, rx) = sync_channel(8);
        {
            let exec = Executor::new(1);
            for i in 0..8 {
                let tx = tx.clone();
                exec.execute(move || tx.send(i).unwrap()).unwrap();
            }
        }
        assert_eq!(rx.try_iter().count(), 8);
    }
}
