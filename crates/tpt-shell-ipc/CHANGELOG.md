# Changelog

All notable changes to `tpt-shell-ipc` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- IPC envelope version 1: parsing with `Limits`, and the response and event builders.
- `Router` and `Caller`, with command errors that carry a `TS####` code and optional structured details. Invalid arguments report the offending path.
- Commands can be registered behind a capability (`register_guarded`). `handle_authorized` checks the caller's window and origin grant before the handler runs, and asks for consent when a grant needs it. Guarded commands are refused on the unauthorised entry point.
- `Router::capability_of(cmd)`, used by the audit record.
- Jobs with progress, cancellation and typed TypeScript events.
- `job.cancel` requires the `job.cancel` capability and cancels only jobs the calling window started.
- Typed contract, contract diff and the TypeScript client generator.
- `ScopedPath` and `DeviceId` typed arguments.
- Per-caller rate limits, and streamed results.
- Blob payloads passed by reference.
- `Jobs::list` and `JobInfo` report each job's state, latest progress and owning window, with the last 20 finished jobs (`RECENT_LIMIT`).
- `Jobs::drain_with_owner` and `Router::drain_job_events_by_window` pair each job event with the window that started the job.
