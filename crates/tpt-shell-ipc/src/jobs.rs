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

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use crate::router::CommandError;

/// Identifies one job within a [`Jobs`] runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JobId(pub u64);

/// Something a job reports.
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
    pub fn job(&self) -> JobId {
        match self {
            JobEvent::Progress { job, .. }
            | JobEvent::Result { job, .. }
            | JobEvent::Failed { job, .. }
            | JobEvent::Cancelled { job } => *job,
        }
    }

    /// Whether this is the job's last event.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, JobEvent::Progress { .. })
    }

    /// The envelope event sent to the page, for example `{"event":"job.progress","data":{...}}`.
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
pub struct JobContext {
    job: JobId,
    cancel: Arc<AtomicBool>,
    events: Sender<JobEvent>,
}

impl JobContext {
    /// Reports progress. `total` is the amount of work; `done` is how much is finished.
    pub fn progress(&self, done: u32, total: u32) {
        let _ = self.events.send(JobEvent::Progress {
            job: self.job,
            done,
            total,
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Why a job could not start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The limit on running jobs is reached.
    TooMany { limit: usize },
}

struct Running {
    cancel: Arc<AtomicBool>,
    finished: bool,
}

/// Runs jobs and collects their events.
pub struct Jobs {
    limit: usize,
    next: u64,
    running: HashMap<JobId, Running>,
    events_tx: Sender<JobEvent>,
    events_rx: Receiver<JobEvent>,
}

impl Jobs {
    /// `limit` is the most jobs that may be running at once.
    pub fn new(limit: usize) -> Self {
        let (events_tx, events_rx) = channel();
        Jobs {
            limit,
            next: 1,
            running: HashMap::new(),
            events_tx,
            events_rx,
        }
    }

    /// Starts `body` on a new thread and returns its id.
    pub fn start<F>(&mut self, body: F) -> Result<JobId, StartError>
    where
        F: FnOnce(&JobContext) -> Result<Value, CommandError> + Send + 'static,
    {
        self.reap();
        if self.running.len() >= self.limit {
            return Err(StartError::TooMany { limit: self.limit });
        }
        let id = JobId(self.next);
        self.next += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.running.insert(
            id,
            Running {
                cancel: cancel.clone(),
                finished: false,
            },
        );
        let ctx = JobContext {
            job: id,
            cancel: cancel.clone(),
            events: self.events_tx.clone(),
        };
        let events = self.events_tx.clone();
        thread::spawn(move || {
            let outcome = body(&ctx);
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
    pub fn cancel(&mut self, id: JobId) -> bool {
        match self.running.get(&id) {
            Some(job) if !job.finished => {
                job.cancel.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        }
    }

    /// Collects every event reported so far, in the order they were sent. Call this on the UI
    /// thread; it never blocks.
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
    pub fn active(&self) -> usize {
        self.running.values().filter(|j| !j.finished).count()
    }

    fn reap(&mut self) {
        self.running.retain(|_, j| !j.finished);
    }
}

/// Starts `body` on a runner shared by a router's job commands (see `Router::jobs`) and returns
/// the page's answer, `{"job": id}`. A refused start is `JOB_LIMIT`, with a hint.
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
        let mut router = Router::new();
        let jobs = router.jobs();
        let answer = start_shared(&jobs, |ctx| {
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Err(CommandError::new("JOB_STOPPED", "stopped"))
        })
        .unwrap();
        let request =
            json!({ "v": 1, "id": 1, "cmd": "job.cancel", "args": { "job": answer["job"] } });
        let reply = router.handle(request.to_string().as_bytes(), &crate::Limits::default());
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
