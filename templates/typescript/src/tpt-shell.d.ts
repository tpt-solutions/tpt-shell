// The runtime object that tpt-shell adds at document start (js/runtime/tpt-shell.js).
interface Window {
  tpt: {
    invoke(cmd: string, args?: unknown): Promise<unknown>;
    on(event: string, handler: (data: unknown) => void): () => void;
    readonly caps: { readonly engine: "webview2"; readonly version: string | null };
  };
}
