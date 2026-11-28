# Changelog

All notable changes to `tpt-shell-devtools` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Developer overlay page, opened with Ctrl+Shift+D in debug builds.
- The overlay shows the IPC log, jobs, capabilities, the audit tail, a contract explorer and engine information.
- `Snapshot` and the `Inspect` trait, so the app supplies the state and this crate does not reach into the runtime.
- `audit_rows` reads the last `AUDIT_TAIL` (200) audit lines.
- The overlay page is built with no external resources.
- `enabled()` and `page_if_enabled()` return nothing in release builds.
- Accessibility panel: `a11y::check` runs the `tpt-shell-test` rules on the page's elements that have an `id`, and `a11y::to_json` reports the findings. Partial: some rules are heuristics, as the module documentation says.
- Jobs panel lists the app router's running jobs and the last 20 finished ones.
- The engine version is shown when the WebView2 loader reports it.
- The timeline's `dom_at_cursor` returns the page snapshot from the latest input event before the cursor.
- `overlay` feature, on by default for this crate. The facade enables it only through its `devtools` feature.
