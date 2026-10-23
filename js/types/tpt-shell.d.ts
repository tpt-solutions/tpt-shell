// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions
//
// Declarations for the page runtime that tpt-shell adds at document start (js/runtime/tpt-shell.js).
// A global script declaration: it has no imports, so a page includes it with a triple-slash
// reference or by listing it in tsconfig.json.

interface TptJob {
  readonly id: number;
  /** Calls the job.cancel command for this job. */
  cancel(): Promise<unknown>;
  /** Receives the events of this job only. Returns a function that unsubscribes. */
  on(event: string, handler: (data: unknown) => void): () => void;
}

interface TptNormalizeOptions {
  /** Adds a viewport meta tag when the page has none. */
  viewport?: boolean;
  /** Sets a default font stack, or the given one, on the root element. */
  font?: boolean | string;
  /** Reserves the scrollbar gutter so the layout does not shift. */
  scrollbars?: boolean;
  /** Makes `data-tpt-drag` areas draggable; `data-tpt-no-drag` inside them is excluded. */
  dragRegion?: boolean;
  /** Defines the `--tpt-safe-*` custom properties from the safe-area insets. */
  safeArea?: boolean;
}

interface TptTauriListenEvent<T = unknown> {
  event: string;
  payload: T;
}

interface TptTauriShim {
  invoke(cmd: string, args?: unknown): Promise<unknown>;
  event: {
    listen<T = unknown>(
      event: string,
      handler: (e: TptTauriListenEvent<T>) => void
    ): Promise<() => void>;
  };
  /** Sets window.__TAURI__ and window.__TAURI_INTERNALS__. Opt-in. */
  install(): void;
}

interface Window {
  tpt: {
    invoke(cmd: string, args?: unknown): Promise<unknown>;
    on(event: string, handler: (data: unknown) => void): () => void;
    job(id: number): TptJob;
    normalize(options?: TptNormalizeOptions): boolean;
    compat: { tauri: TptTauriShim };
    readonly caps: {
      readonly engine: "webview2";
      /** The WebView2 version from the user agent, or null when it does not name one. */
      readonly version: string | null;
      /** The runtime APIs this build provides. Check it before calling an API. */
      readonly apis: readonly string[];
    };
  };
  __TAURI__?: {
    core: { invoke(cmd: string, args?: unknown): Promise<unknown> };
    event: TptTauriShim["event"];
  };
  __TAURI_INTERNALS__?: { invoke(cmd: string, args?: unknown): Promise<unknown> };
}
