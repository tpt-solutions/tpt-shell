# tpt-shell-runtime

The page-side JavaScript that tpt-shell adds to a window: `window.tpt`, the mock for a plain
browser tab, the optional `<tpt-progress>` element, and TypeScript declarations.

This package is **private** (`"private": true`). It is not published to npm. Pages that ship with
an app get the runtime from the shell, not from this package, so nothing here is needed at run time.

## Files

| Path | What it is |
|---|---|
| `runtime/tpt-shell.js` | The runtime. The shell injects it at document start. |
| `runtime/tpt-shell-mock.js` | `window.tpt` for `tpt-shell dev` in a browser tab. Does nothing inside a shell window. |
| `components/tpt-progress.js` | Optional `<tpt-progress>` element for a job. Not counted in the runtime budget. |
| `types/tpt-shell.d.ts` | Global TypeScript declarations for `window.tpt` and the Tauri shim. |

## Size budget

`tools/check-runtime-size.sh` keeps `runtime/` under 4 KB gzipped in total (spec §7.2).

## Checks

```sh
npm test            # node --test: runtime, progress element and replay shim tests
npm pack --dry-run  # lists the files a package would contain; publishes nothing
```

Publishing is out of scope here. Removing `"private": true` is a release decision (todo M11b).
