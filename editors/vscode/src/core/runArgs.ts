// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The arguments for a run configuration, built the way the CLI reads them (see the usage text in
// tpt-shell-cli): `dev <directory> [--port <port>]` and `build [directory]`.

export interface RunDefinition {
  command: string;
  directory?: string;
  port?: number;
}

export type RunArgs = { ok: true; args: string[]; background: boolean } | { ok: false; message: string };

export function runArgs(definition: RunDefinition, defaultDirectory: string): RunArgs {
  const directory = definition.directory && definition.directory.trim() !== "" ? definition.directory : defaultDirectory;
  if (definition.command === "build") {
    return { ok: true, args: ["build", directory], background: false };
  }
  if (definition.command === "dev") {
    const port = definition.port ?? 8080;
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      return { ok: false, message: `port must be a whole number from 1 to 65535, not ${port}` };
    }
    return { ok: true, args: ["dev", directory, "--port", String(port)], background: true };
  }
  return { ok: false, message: `command must be "dev" or "build", not "${definition.command}"` };
}
