# Changelog

All notable changes to `tpt-shell-test` are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). The crate is not published yet. It will follow Semantic Versioning from v1.0.0.

## [Unreleased]

### Added
- Screenshot comparer over raw RGBA, with a YIQ-weighted distance, ignore and per-region tolerance regions, and a limit on the share of changed pixels.
- PNG decoder for the comparer.
- Merge of screenshots from several CI jobs, with an HTML report.
- Accessibility rules: contrast against the WCAG AA minimums and the large-text rule, a 24 px target-size check, unnamed form inputs, focus order, keyboard traps and reduced motion, with seeded fixtures.
- Comparison of the DOM with the native accessibility tree.
- Test driver behind the `test-driver` feature, with a file-based control channel and a per-run token. It opens no network port.
- Artifact collection for `tpt-shell test`.
- `tpt-shell visual compare` writes the HTML report and exits with an error when the limits are exceeded.
- Error codes `TS0301` to `TS0305`.
