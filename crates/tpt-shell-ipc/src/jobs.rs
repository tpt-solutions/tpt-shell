// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Jobs API (spec M3): long-running commands that report progress, can be cancelled, and end
//! with a result or a failure.
//!
//! A job runs on its own thread. It reports through a [`JobContext`] and checks for
//! cancellation between steps. The UI thread calls [`Jobs::drain`] to collect the events and
//! forwards them to the page as envelope events (see [`JobEvent::to_envelope`]).
//!
//! Cancellation is cooperative: a job that never checks [`JobContext::is_cancelled`] runs to
//! the end. Each job reports exactly one terminal event: `Result`, `Failed` or `Cancelled`.
//!
//! A job can also stream items (see [`Jobs::start_streamed`]). Its items go through the stream
//! channel in [`crate::stream`], so a receiver that stops taking them stops the job, and the
//! stream closes when the job finishes or is cancelled.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::router::CommandError;
use crate::stream::{SendError, Sender as StreamSender};

/// Runs `f` with the stream locked. A poisoned lock is still used: the stream holds only queue
/// counts and chunks, which a panicking job cannot leave half-updated in a way that matters.
fn with_stream<T>(stream: &Mutex<StreamSender>, f: impl FnOnce(&mut StreamSender) -> T) -> T {
    let mut guard = stream.lock().unwrap_or_else(PoisonError::into_inner);
    f(&mut guard)
}

/// Identifies one job within a [`Jobs`] runner.
///
/// Support: Full.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

/// Something a job reports.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq)]
pub enum JobEvent {
    Progress {
        job: JobId,
        done: u32,
        total: u32,
    },
    Result {
        job: JobId,
        data: Value,
    },
    Failed {
        job: JobId,
        code: String,
        message: String,
    },
    Cancelled {
        job: JobId,
    },
}

impl JobEvent {
    /// Support: Full.
    pub fn job(&self) -> JobId {
        match self {
            JobEvent::Progress { job, .. }
            | JobEvent::Result { job, .. }
            | JobEvent::Failed { job, .. }
            | JobEvent::Cancelled { job } => *job,
        }
    }

    /// Whether this is the job's last event.
    ///
    /// Support: Full.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, JobEvent::Progress { .. })
    }

    /// The envelope event sent to the page, for example `{"event":"job.progress","data":{...}}`.
    ///
    /// Support: Full.
    pub fn to_envelope(&self) -> Value {
        match self {
            JobEvent::Progress { job, done, total } => crate::event(
                "job.progress",
                json!({ "job": job.0, "done": done, "total": total }),
            ),
            JobEvent::Result { job, data } => {
                crate::event("job.result", json!({ "job": job.0, "data": data }))
            }
            JobEvent::Failed { job, code, message } => crate::event(
                "job.failed",
                json!({ "job": job.0, "code": code, "message": message }),
            ),
            JobEvent::Cancelled { job } => crate::event("job.cancelled", json!({ "job": job.0 })),
        }
    }
}

/// Handed to a job's body.
///
/// Support: Full.
pub struct JobContext {
    job: JobId,
    cancel: Arc<AtomicBool>,
    events: Sender<JobEvent>,
    stream: Option<Arc<Mutex<StreamSender>>>,
}

impl JobContext {
    /// Queues one item for the page's stream, spending one credit. Returns at once: a refused
    /// send is `NoCredit` (wait for a grant, or use [`JobContext::send_item_wait`]) and a job
    /// that was not started with a stream gets `Closed`.
    ///
    /// Support: Partial. The stream is in Rust only; carrying it to a page needs the platform
    /// transport, which is not built.
    pub fn send_item(&self, chunk: Vec<u8>) -> Result<(), SendError> {
        match &self.stream {
            Some(stream) => with_stream(stream, |s| s.send(chunk)),
            None => Err(SendError::Closed),
        }
    }

    /// Queues one item, waiting while the receiver has granted no credit. The wait ends when
    /// credit arrives (the item is sent) or the stream closes, which happens when the job is
    /// cancelled or finishes (`Closed` is returned).
    ///
    /// Support: Partial. The wait polls every millisecond; the stream is in Rust only, as for
    /// [`JobContext::send_item`].
    pub fn send_item_wait(&self, chunk: Vec<u8>) -> Result<(), SendError> {
        let Some(stream) = &self.stream else {
            return Err(SendError::Closed);
        };
        let mut pending = Some(chunk);
        loop {
            let sent = with_stream(stream, |s| {
                if s.is_closed() {
                    return Err(SendError::Closed);
                }
                if s.credit() == 0 {
                    return Ok(false);
                }
                match pending.take() {
                    Some(chunk) => s.send(chunk).map(|()| true),
                    None => Ok(true),
                }
            });
            match sent {
                Ok(true) => return Ok(()),
                Ok(false) => thread::sleep(Duration::from_millis(1)),
                Err(e) => return Err(e),
            }
        }
    }

    /// Reports progress. `total` is the amount of work; `done` is how much is finished.
    ///
    /// Support: Full.
    pub fn progress(&self, done: u32, total: u32) {
        let _ = self.events.send(JobEvent::Progress {
            job: self.job,
            done,
            total,
        });
    }

    /// Support: Full.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Why a job could not start.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The limit on running jobs is reached.
    TooMany { limit: usize },
}

struct Running {
    cancel: Arc<AtomicBool>,
    finished: bool,
    /// The window that started the job. Empty when no window was calling.
    owner: String,
    /// The job's item stream, when it was started with one.
    stream: Option<Arc<Mutex<StreamSender>>>,
}

/// Whether a job's stream still holds items the receiver has not taken. A finished job is kept
/// until they are taken, so the page can still read them.
fn holds_items(job: &Running) -> bool {
    job.stream
        .as_ref()
        .is_some_and(|s| with_stream(s, |s| s.queued() > 0))
}

/// Asks a job to stop and closes its stream, so a job waiting for credit returns at once.
fn request_cancel(job: &Running) {
    job.cancel.store(true, Ordering::SeqCst);
    if let Some(stream) = &job.stream {
        with_stream(stream, |s| s.close());
    }
}

/// Runs jobs and collects their events.
///
/// Support: Partial. Cancellation is cooperative: a job that never checks is_cancelled runs to the
/// end.
pub struct Jobs {
    limit: usize,
    next: u64,
    running: HashMap<JobId, Running>,
    events_tx: Sender<JobEvent>,
    events_rx: Receiver<JobEvent>,
    /// The window whose call is running now. A job started meanwhile belongs to it.
    owner: String,
}

impl Jobs {
    /// `limit` is the most jobs that may be running at once.
    ///
    /// Support: Full.
    pub fn new(limit: usize) -> Self {
        let (events_tx, events_rx) = channel();
        Jobs {
            limit,
            next: 1,
            running: HashMap::new(),
            events_tx,
            events_rx,
            owner: String::new(),
        }
    }

    /// Sets the window that owns jobs started from now on. The router sets it around each
    /// guarded command, and clears it with an empty name afterwards.
    ///
    /// Support: Full.
    pub fn set_owner(&mut self, window: &str) {
        self.owner = window.to_string();
    }

    /// The window that started job `id`, while the job is known.
    ///
    /// Support: Full.
    pub fn owner_of(&self, id: JobId) -> Option<&str> {
        self.running.get(&id).map(|job| job.owner.as_str())
    }

    /// Starts `body` on a new thread and returns its id.
    ///
    /// Support: Full.
    pub fn start<F>(&mut self, body: F) -> Result<JobId, StartError>
    where
        F: FnOnce(&JobContext) -> Result<Value, CommandError> + Send + 'static,
    {
        self.spawn(None, body)
    }

    /// Starts `body` like [`Jobs::start`], and gives it an item stream to the page. The stream
    /// opens with `initial_credit` items in flight, and each item may be at most `max_chunk`
    /// bytes. Items are taken with [`Jobs::stream_take`] and credit is returned with
    /// [`Jobs::stream_grant`]. The stream closes when the job finishes or is cancelled.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn start_streamed<F>(
        &mut self,
        initial_credit: u32,
        max_chunk: usize,
        body: F,
    ) -> Result<JobId, StartError>
    where
        F: FnOnce(&JobContext) -> Result<Value, CommandError> + Send + 'static,
    {
        self.spawn(Some((initial_credit, max_chunk)), body)
    }

    fn spawn<F>(&mut self, stream: Option<(u32, usize)>, body: F) -> Result<JobId, StartError>
    where
        F: FnOnce(&JobContext) -> Result<Value, CommandError> + Send + 'static,
    {
        self.reap();
        if self.active() >= self.limit {
            return Err(StartError::TooMany { limit: self.limit });
        }
        let id = JobId(self.next);
        self.next += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let stream =
            stream.map(|(credit, max)| Arc::new(Mutex::new(StreamSender::new(credit, max))));
        self.running.insert(
            id,
            Running {
                cancel: cancel.clone(),
                finished: false,
                owner: self.owner.clone(),
                stream: stream.clone(),
            },
        );
        let ctx = JobContext {
            job: id,
            cancel,
            events: self.events_tx.clone(),
            stream,
        };
        let events = self.events_tx.clone();
        thread::spawn(move || {
            let outcome = body(&ctx);
            if let Some(stream) = &ctx.stream {
                with_stream(stream, |s| s.close());
            }
            let terminal = match outcome {
                Ok(data) => JobEvent::Result { job: id, data },
                Err(_) if ctx.is_cancelled() => JobEvent::Cancelled { job: id },
                Err(e) => JobEvent::Failed {
                    job: id,
                    code: e.code,
                    message: e.message,
                },
            };
            let _ = events.send(terminal);
        });
        Ok(id)
    }

    /// Asks a job to stop. Returns `false` when the id is unknown or the job already ended.
    ///
    /// Support: Partial. Cancellation is cooperative: a job that never checks is_cancelled runs to
    /// the end.
    pub fn cancel(&mut self, id: JobId) -> bool {
        match self.running.get(&id) {
            Some(job) if !job.finished => {
                request_cancel(job);
                true
            }
            _ => false,
        }
    }

    /// Asks a job to stop, but only when `window` started it. Returns `false` for any other
    /// window, an unknown id, or a job that already ended.
    ///
    /// Support: Partial. Cancellation is cooperative: a job that never checks is_cancelled runs to
    /// the end.
    pub fn cancel_owned(&mut self, window: &str, id: JobId) -> bool {
        match self.running.get(&id) {
            Some(job) if !job.finished && !window.is_empty() && job.owner == window => {
                request_cancel(job);
                true
            }
            _ => false,
        }
    }

    /// Collects every event reported so far, in the order they were sent. Call this on the UI
    /// thread; it never blocks.
    ///
    /// Support: Full.
    pub fn drain(&mut self) -> Vec<JobEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            if event.is_terminal() {
                if let Some(job) = self.running.get_mut(&event.job()) {
                    job.finished = true;
                }
            }
            out.push(event);
        }
        self.reap();
        out
    }

    /// Jobs that have not reported a terminal event yet.
    ///
    /// Support: Full.
    pub fn active(&self) -> usize {
        self.running.values().filter(|j| !j.finished).count()
    }

    /// Takes the next item job `id` queued, if any. Taking an item does not return credit; only
    /// [`Jobs::stream_grant`] does. A finished job's last items can still be taken.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn stream_take(&mut self, id: JobId) -> Option<Vec<u8>> {
        let chunk = self
            .running
            .get(&id)?
            .stream
            .as_ref()
            .and_then(|s| with_stream(s, |s| s.take()));
        self.reap();
        chunk
    }

    /// Like [`Jobs::stream_take`], but only for the window that started the job. Any other
    /// window gets `None`.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn stream_take_owned(&mut self, window: &str, id: JobId) -> Option<Vec<u8>> {
        if window.is_empty() || self.owner_of(id)? != window {
            return None;
        }
        self.stream_take(id)
    }

    /// Returns `n` items of credit to job `id`'s stream. Returns `false` when the job is unknown
    /// or has no stream. Credit for a closed stream is accepted and has no effect, since the
    /// receiver may still be taking the items queued before the close.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn stream_grant(&mut self, id: JobId, n: u32) -> bool {
        match self.running.get(&id).and_then(|j| j.stream.as_ref()) {
            Some(stream) => {
                with_stream(stream, |s| s.grant(n));
                true
            }
            None => false,
        }
    }

    /// The number of items job `id`'s stream holds that the receiver has not taken yet. `None`
    /// once the job is gone.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn stream_queued(&self, id: JobId) -> Option<usize> {
        let stream = self.running.get(&id)?.stream.as_ref()?;
        Some(with_stream(stream, |s| s.queued()))
    }

    /// Whether job `id` has a stream that is still open for items.
    ///
    /// Support: Partial. The stream runs in Rust; the page transport for it is not built.
    pub fn stream_is_open(&self, id: JobId) -> bool {
        self.running
            .get(&id)
            .and_then(|j| j.stream.as_ref())
            .is_some_and(|s| !with_stream(s, |s| s.is_closed()))
    }

    /// Removes finished jobs, except those whose stream still holds items.
    fn reap(&mut self) {
        self.running.retain(|_, j| !j.finished || holds_items(j));
    }
}

/// Starts `body` on a runner shared by a router's job commands (see `Router::jobs`) and returns
/// the page's answer, `{"job": id}`. A refused start is `JOB_LIMIT`, with a hint.
///
/// Support: Full.
pub fn start_shared<F>(jobs: &Rc<RefCell<Jobs>>, body: F) -> Result<Value, CommandError>
where
    F: FnOnce(&JobContext) -> Result<Value, CommandError> + Send + 'static,
{
    let id = jobs.borrow_mut().start(body).map_err(|e| match e {
        StartError::TooMany { limit } => CommandError::new(
            "JOB_LIMIT",
            &format!("{limit} jobs are already running; wait for one to finish or cancel it"),
        ),
    })?;
    Ok(json!({ "job": id.0 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::Router;
    use std::time::{Duration, Instant};

    #[test]
    fn a_shared_runner_starts_jobs_through_the_router() {
        let mut router = Router::new();
        let jobs = router.jobs();
        let answer = start_shared(&jobs, |ctx| {
            ctx.progress(1, 1);
            Ok(json!("done"))
        })
        .unwrap();
        let id = answer["job"].as_u64().unwrap();
        let mut seen = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10)
            && !seen.iter().any(|e: &Value| e["event"] == "job.result")
        {
            seen.extend(router.drain_job_events());
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(seen[0]["event"], "job.progress");
        assert_eq!(seen[0]["data"]["job"], id);
        assert!(seen.iter().any(|e| e["event"] == "job.result"));
    }

    #[test]
    fn job_cancel_is_a_command_that_stops_a_job() {
        use tpt_shell_capabilities::{Engine, Grant, Scope};

        struct Decline;
        impl tpt_shell_capabilities::ConsentPrompt for Decline {
            fn ask(&mut self, _window: &str, _capability: &str) -> bool {
                false
            }
        }

        let mut router = Router::new();
        let jobs = router.jobs();
        // The job belongs to window "main", the one whose call starts it.
        jobs.borrow_mut().set_owner("main");
        let answer = start_shared(&jobs, |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(CommandError::new("JOB_STOPPED", "stopped"))
        })
        .unwrap();
        jobs.borrow_mut().set_owner("");
        // job.cancel is guarded, so it needs a grant for the calling window.
        let mut engine = Engine::new();
        engine
            .add_grant(Grant {
                capability: "job.cancel".into(),
                window: "main".into(),
                origin: "https://tpt-app.localhost".into(),
                scope: Scope::Unrestricted,
                consent: false,
            })
            .unwrap();
        let request =
            json!({ "v": 1, "id": 1, "cmd": "job.cancel", "args": { "job": answer["job"] } });
        let reply = router.handle_authorized(
            &crate::Caller {
                window: "main".into(),
                origin: "https://tpt-app.localhost".into(),
            },
            &mut engine,
            &mut Decline,
            request.to_string().as_bytes(),
            &crate::Limits::default(),
        );
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["data"]["cancelled"], true);
    }

    #[test]
    fn a_refused_start_names_the_limit() {
        let mut router = Router::new();
        let jobs = router.jobs();
        for _ in 0..crate::router::DEFAULT_JOB_LIMIT {
            start_shared(&jobs, |ctx| {
                while !ctx.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(CommandError::new("JOB_STOPPED", "stopped"))
            })
            .unwrap();
        }
        let err = start_shared(&jobs, |_| Ok(json!(null))).unwrap_err();
        assert_eq!(err.code, "JOB_LIMIT");
        for id in 1..=crate::router::DEFAULT_JOB_LIMIT as u64 {
            assert!(jobs.borrow_mut().cancel(JobId(id)));
        }
    }

    /// Drains until `done` is true or the timeout passes, and returns everything collected.
    fn collect_until(jobs: &mut Jobs, mut done: impl FnMut(&[JobEvent]) -> bool) -> Vec<JobEvent> {
        let start = Instant::now();
        let mut all = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            all.extend(jobs.drain());
            if done(&all) {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        all
    }

    #[test]
    fn progress_then_result_in_order() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start(|ctx| {
                ctx.progress(1, 2);
                ctx.progress(2, 2);
                Ok(json!({ "files": 2 }))
            })
            .unwrap();
        let events = collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert_eq!(
            events,
            vec![
                JobEvent::Progress {
                    job: id,
                    done: 1,
                    total: 2
                },
                JobEvent::Progress {
                    job: id,
                    done: 2,
                    total: 2
                },
                JobEvent::Result {
                    job: id,
                    data: json!({ "files": 2 })
                },
            ]
        );
    }

    #[test]
    fn a_failing_job_reports_its_code() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start(|_| Err(CommandError::new("APP_DISK_FULL", "no space left")))
            .unwrap();
        let events = collect_until(&mut jobs, |e| !e.is_empty());
        assert_eq!(
            events,
            vec![JobEvent::Failed {
                job: id,
                code: "APP_DISK_FULL".into(),
                message: "no space left".into(),
            }]
        );
    }

    #[test]
    fn cancel_stops_a_cooperative_job() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start(|ctx| {
                for step in 0..10_000u32 {
                    if ctx.is_cancelled() {
                        return Err(CommandError::new("JOB_STOPPED", "stopped"));
                    }
                    ctx.progress(step, 10_000);
                    thread::sleep(Duration::from_millis(1));
                }
                Ok(json!(null))
            })
            .unwrap();
        // Wait for the first progress report so the job is really running.
        let first = collect_until(&mut jobs, |e| !e.is_empty());
        assert!(matches!(first[0], JobEvent::Progress { .. }));
        assert!(jobs.cancel(id));
        let rest = collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(rest.contains(&JobEvent::Cancelled { job: id }), "{rest:?}");
        assert!(!rest.iter().any(|e| matches!(e, JobEvent::Result { .. })));
    }

    #[test]
    fn cancel_is_refused_for_finished_or_unknown_jobs() {
        let mut jobs = Jobs::new(4);
        let id = jobs.start(|_| Ok(json!(1))).unwrap();
        collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(!jobs.cancel(id));
        assert!(!jobs.cancel(JobId(999)));
    }

    #[test]
    fn the_running_limit_is_enforced_and_frees_up_after_a_job_ends() {
        let mut jobs = Jobs::new(1);
        let slow = jobs
            .start(|ctx| {
                while !ctx.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(CommandError::new("JOB_STOPPED", "stopped"))
            })
            .unwrap();
        assert_eq!(
            jobs.start(|_| Ok(json!(null))),
            Err(StartError::TooMany { limit: 1 })
        );
        jobs.cancel(slow);
        collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(jobs.start(|_| Ok(json!(null))).is_ok());
    }

    #[test]
    fn a_streamed_job_delivers_its_items_in_order_under_backpressure() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start_streamed(2, 16, |ctx| {
                for i in 0..6u8 {
                    ctx.send_item_wait(vec![i])
                        .map_err(|e| CommandError::new("STREAM_CLOSED", &e.to_string()))?;
                }
                Ok(json!("done"))
            })
            .unwrap();
        let mut got = Vec::new();
        let mut events = Vec::new();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) && !events.iter().any(JobEvent::is_terminal)
        {
            // Credit is spent on send and returned only by a grant, so at most the initial
            // credit of items can sit in the queue.
            assert!(jobs.stream_queued(id).unwrap_or(0) <= 2);
            while let Some(chunk) = jobs.stream_take(id) {
                got.push(chunk);
                assert!(jobs.stream_grant(id, 1));
            }
            events.extend(jobs.drain());
            thread::sleep(Duration::from_millis(1));
        }
        // Items sent just before the job finished may still be queued.
        while let Some(chunk) = jobs.stream_take(id) {
            got.push(chunk);
        }
        assert_eq!(got, (0..6u8).map(|i| vec![i]).collect::<Vec<_>>());
        assert!(events.contains(&JobEvent::Result {
            job: id,
            data: json!("done")
        }));
        // Once the items are taken, the finished job is removed.
        assert_eq!(jobs.stream_queued(id), None);
    }

    #[test]
    fn send_item_is_refused_without_credit_and_keeps_the_queued_item() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start_streamed(1, 8, |ctx| {
                let first = ctx.send_item(vec![1]);
                let second = ctx.send_item(vec![2]);
                Ok(json!([format!("{first:?}"), format!("{second:?}")]))
            })
            .unwrap();
        let events = collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(events.contains(&JobEvent::Result {
            job: id,
            data: json!(["Ok(())", "Err(NoCredit)"]),
        }));
        assert_eq!(jobs.stream_queued(id), Some(1));
        assert_eq!(jobs.stream_take(id), Some(vec![1]));
    }

    #[test]
    fn a_job_without_a_stream_gets_closed_on_every_stream_call() {
        let mut jobs = Jobs::new(4);
        let id = jobs
            .start(|ctx| Ok(json!(format!("{:?}", ctx.send_item(vec![9])))))
            .unwrap();
        let events = collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(events.contains(&JobEvent::Result {
            job: id,
            data: json!("Err(Closed)"),
        }));
        assert_eq!(jobs.stream_take(id), None);
        assert!(!jobs.stream_grant(id, 1));
        assert!(!jobs.stream_is_open(id));
    }

    #[test]
    fn cancel_closes_the_stream_and_releases_a_waiting_sender() {
        let mut jobs = Jobs::new(4);
        // No credit at all: the job waits in send_item_wait until the stream closes.
        let id = jobs
            .start_streamed(0, 8, |ctx| {
                let sent = ctx.send_item_wait(vec![1]);
                Ok(json!(format!("{sent:?}")))
            })
            .unwrap();
        assert!(jobs.stream_is_open(id));
        assert!(jobs.cancel(id));
        // Closed at once, before the job has even noticed.
        assert!(!jobs.stream_is_open(id));
        let events = collect_until(&mut jobs, |e| e.iter().any(JobEvent::is_terminal));
        assert!(events.contains(&JobEvent::Result {
            job: id,
            data: json!("Err(Closed)"),
        }));
    }

    #[test]
    fn a_streamed_job_is_limited_like_any_other() {
        let mut jobs = Jobs::new(1);
        let slow = jobs
            .start_streamed(1, 8, |ctx| {
                while !ctx.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(CommandError::new("JOB_STOPPED", "stopped"))
            })
            .unwrap();
        assert_eq!(
            jobs.start_streamed(1, 8, |_| Ok(json!(null))),
            Err(StartError::TooMany { limit: 1 })
        );
        jobs.cancel(slow);
    }

    #[test]
    fn events_map_to_envelope_events() {
        let id = JobId(7);
        assert_eq!(
            JobEvent::Progress {
                job: id,
                done: 1,
                total: 4
            }
            .to_envelope(),
            json!({ "event": "job.progress", "data": { "job": 7, "done": 1, "total": 4 } })
        );
        assert_eq!(
            JobEvent::Cancelled { job: id }.to_envelope(),
            json!({ "event": "job.cancelled", "data": { "job": 7 } })
        );
    }
}

/// Runs jobs through the fake window backend. The page's commands arrive as messages to a fake
/// window, and the job events are posted back to that window as messages, as the shell does.
#[cfg(test)]
mod fake_backend_tests {
    use super::*;
    use crate::router::Router;
    use crate::schema::Schema;
    use crate::{Caller, Limits, Target};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Instant;
    use tpt_shell_capabilities::{ConsentPrompt, Engine, Grant, Scope};
    use tpt_shell_core::backend::{MessageHandler, WebviewBackend, WindowBackend, WindowSpec};
    use tpt_shell_core::fake::{FakeBackend, FakeId};
    use tpt_shell_core::LogicalSize;

    const ORIGIN: &str = "https://tpt-app.localhost";

    struct Decline;
    impl ConsentPrompt for Decline {
        fn ask(&mut self, _window: &str, _capability: &str) -> bool {
            false
        }
    }

    fn grant(window: &str, capability: &str) -> Grant {
        Grant {
            capability: capability.into(),
            window: window.into(),
            origin: ORIGIN.into(),
            scope: Scope::Unrestricted,
            consent: false,
        }
    }

    fn spec(name: &str) -> WindowSpec {
        WindowSpec {
            name: name.into(),
            title: name.into(),
            size: LogicalSize {
                width: 800.0,
                height: 600.0,
            },
            allow_navigation: Vec::new(),
        }
    }

    /// A router with two job commands. `start_work` reports two steps and returns a result.
    /// `start_wait` reports steps until it is cancelled (or ten seconds pass).
    fn router() -> Router {
        let mut router = Router::new();
        router.register_job_guarded(
            "start_work",
            "job.start",
            Schema::Struct(&[]),
            |_| Target::None,
            |_, jobs| {
                start_shared(jobs, |ctx| {
                    ctx.progress(1, 2);
                    ctx.progress(2, 2);
                    Ok(json!("done"))
                })
            },
        );
        router.register_job_guarded(
            "start_wait",
            "job.start",
            Schema::Struct(&[]),
            |_| Target::None,
            |_, jobs| {
                start_shared(jobs, |ctx| {
                    let until = Instant::now() + Duration::from_secs(10);
                    let mut step = 0;
                    while !ctx.is_cancelled() && Instant::now() < until {
                        step += 1;
                        ctx.progress(step, 1000);
                        thread::sleep(Duration::from_millis(5));
                    }
                    if ctx.is_cancelled() {
                        Err(CommandError::new("JOB_STOPPED", "stopped"))
                    } else {
                        Ok(json!("timed out"))
                    }
                })
            },
        );
        router
    }

    /// The page handler of a window: each message is a command from that window.
    fn page(router: Rc<Router>, engine: Rc<RefCell<Engine>>, window: &str) -> MessageHandler {
        let caller = Caller {
            window: window.into(),
            origin: ORIGIN.into(),
        };
        Box::new(move |text| {
            Some(router.handle_authorized(
                &caller,
                &mut engine.borrow_mut(),
                &mut Decline,
                text.as_bytes(),
                &Limits::default(),
            ))
        })
    }

    /// Sends one command from the page and returns the router's reply.
    fn call(b: &mut FakeBackend, window: FakeId, cmd: &str, args: Value) -> Value {
        let text = json!({ "v": 1, "id": 1, "cmd": cmd, "args": args }).to_string();
        let reply = b.page_sends(window, &text).unwrap().expect("a reply");
        serde_json::from_str(&reply).unwrap()
    }

    /// Takes the job events the router has, and posts each one to the window.
    fn pump(router: &Router, b: &mut FakeBackend, window: FakeId) -> Vec<Value> {
        let events = router.drain_job_events();
        for event in &events {
            b.post_message(window, &event.to_string()).unwrap();
        }
        events
    }

    /// Pumps until `done` accepts everything seen so far, or ten seconds pass.
    fn pump_until(
        router: &Router,
        b: &mut FakeBackend,
        window: FakeId,
        mut done: impl FnMut(&[Value]) -> bool,
    ) -> Vec<Value> {
        let start = Instant::now();
        let mut seen = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            seen.extend(pump(router, b, window));
            if done(&seen) {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        seen
    }

    fn has(events: &[Value], name: &str, job: u64) -> bool {
        events
            .iter()
            .any(|e| e["event"] == name && e["data"]["job"] == job)
    }

    #[test]
    fn a_job_runs_through_the_fake_backend_end_to_end() {
        let router = Rc::new(router());
        let mut engine = Engine::new();
        for (window, capability) in [
            ("main", "job.start"),
            ("main", "job.cancel"),
            ("other", "job.cancel"),
        ] {
            engine.add_grant(grant(window, capability)).unwrap();
        }
        let engine = Rc::new(RefCell::new(engine));
        let mut b = FakeBackend::new();
        let main = b.create(&spec("main")).unwrap();
        let other = b.create(&spec("other")).unwrap();
        b.set_message_handler(main, page(router.clone(), engine.clone(), "main"))
            .unwrap();
        b.set_message_handler(other, page(router.clone(), engine.clone(), "other"))
            .unwrap();

        // A job that finishes: its progress and its result reach the window as messages.
        let work = call(&mut b, main, "start_work", json!({}))["data"]["job"]
            .as_u64()
            .unwrap();
        let seen = pump_until(&router, &mut b, main, |e| has(e, "job.result", work));
        assert!(has(&seen, "job.progress", work));
        let result = seen
            .iter()
            .find(|e| e["event"] == "job.result" && e["data"]["job"] == work)
            .unwrap();
        assert_eq!(result["data"]["data"], "done");
        assert!(b
            .window(main)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.contains("job.result")));

        // A job that runs until it is cancelled. Another window may not stop it.
        let wait = call(&mut b, main, "start_wait", json!({}))["data"]["job"]
            .as_u64()
            .unwrap();
        pump_until(&router, &mut b, main, |e| has(e, "job.progress", wait));
        let refused = call(&mut b, other, "job.cancel", json!({ "job": wait }));
        assert_eq!(refused["data"]["cancelled"], false);
        // The window that started it can.
        let cancelled = call(&mut b, main, "job.cancel", json!({ "job": wait }));
        assert_eq!(cancelled["data"]["cancelled"], true);
        let seen = pump_until(&router, &mut b, main, |e| has(e, "job.cancelled", wait));
        assert!(has(&seen, "job.cancelled", wait));
        assert!(!has(&seen, "job.result", wait));
        assert!(b
            .window(main)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.contains("job.cancelled")));
    }
}
