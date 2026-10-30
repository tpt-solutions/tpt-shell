# tpt-shell-test

Test support for tpt-shell apps: a test driver with a file-based control channel, a screenshot comparer with an HTML report, an accessibility rule engine, a native accessibility tree comparison, and collection of test artifacts.

Part of [tpt-shell](https://github.com/tpt-solutions/tpt-shell).

> **Status: pre-alpha.** Not published (`publish = false`, version `0.0.0`). The visual comparer, PNG decoder and accessibility rules are built and tested. The native tree dumps and screenshot capture come from the platform backends.

## Parts

| Module | Contents |
|---|---|
| `driver`, `channel` | The test driver. It is built only with the `test-driver` feature. |
| `visual` | The screenshot comparer. It works on raw RGBA and uses a YIQ-weighted colour distance. It supports ignore regions, per-region tolerance and a limit on the share of changed pixels. |
| `screens` | Merges screenshots from several CI jobs and writes the HTML report. |
| `png` | The PNG decoder that `visual` uses. |
| `a11y` | The accessibility rules. |
| `native` | Compares the DOM with the native accessibility tree. |
| `artifacts` | Collects the artifacts that `tpt-shell test` writes. |

## The control channel

The test driver talks to the app through a directory with a per-run token. It is not a socket or a named pipe, so it opens no network port. Each run gets a new token.

The channel is compiled in only with the `test-driver` feature. A shipped app that does not enable it has no channel code in it.

## Accessibility rules

| Rule | What it checks |
|---|---|
| Contrast | The WCAG AA contrast ratio, with the large-text minimum. |
| Target size | A 24 px minimum for interactive targets. |
| Form labels | Form inputs with no accessible name. |
| Focus order | The order in which focus moves through the page. |
| Keyboard traps | Focus that cannot leave a control. |
| Reduced motion | Motion that ignores the reduced-motion setting. |

Fixtures are seeded, so the same page gives the same results.

## Visual comparison

```sh
tpt-shell visual compare <baseline-dir> <dir> [dir...] --out report.html --tolerance 0.02 --max-changed 0.01
```

The command exits with an error when a screenshot differs by more than the limits. The HTML report shows each difference.

## Usage

```rust
use tpt_shell_test::png;
```

The rule and comparison functions are in their modules. The facade's test support uses them, and most apps run them through the command line tool.

## Error codes

The test driver uses the `TS0301` to `TS0305` range. Run `tpt-shell explain TS0301` for the fix hint.

## Platform support

| Part | Windows | macOS | Linux |
|---|---|---|---|
| Visual comparer, PNG decoder, accessibility rules | Built and tested | Platform-neutral, tested on every host | Platform-neutral, tested on every host |
| Native accessibility tree | Built (UI Automation) | Not built | Not built |
| Screenshot capture | Built on Windows | Not built | Not built |

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## Licence

Dual-licensed under either of [MIT](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/tpt-solutions/tpt-shell/blob/master/LICENSE-APACHE), at your option.

## Contributing

Contributions are accepted as issues only. See [CONTRIBUTING.md](https://github.com/tpt-solutions/tpt-shell/blob/master/CONTRIBUTING.md).
