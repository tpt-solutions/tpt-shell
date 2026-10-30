# Changelog

All notable changes to `tpt-shell-supervisor` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Runs the app as a child process, with a heartbeat file.
- Stops a run that stops beating, or that reaches its scheduled restart.
- Restarts with exponential backoff, and gives up after a crash loop.
- Writes an alert (a command or a file) and a rotating log.
- `Backoff`, `CrashLoopGuard`, `Heartbeat` and `Scheduled`, with decisions that are pure functions of the times given.
- `supervise`, which runs the loop over a `Spawner`.
- Fault-injection tests.
- `install` functions for a LaunchAgent (macOS), systemd user units (Linux) and Task Scheduler (Windows). They write files only and print the commands.
- `tpt-shell install-service` and `uninstall-service` in the command line tool.
