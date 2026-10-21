// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Runs `tpt-shell audit verify` and turns its exit status into a verify status. The CLI owns the
// check: this module does not reimplement the hash chain. Exit codes follow the CLI: 0 verified,
// 1 the checked input has problems (a broken chain or a bad signature), 2 usage or I/O failure.

import { execFile } from "child_process";

export type VerifyStatus = "verified" | "broken" | "error" | "unavailable";

export interface VerifyResult {
  status: VerifyStatus;
  message: string;
}

// `command` is the CLI, and `prefix` are arguments placed before `audit verify`. The prefix lets
// the tests run a stand-in program through the same code path.
export function runVerify(
  command: string,
  prefix: string[],
  file: string,
  publicKey: string | undefined,
  timeoutMs = 60000
): Promise<VerifyResult> {
  const args = [...prefix, "audit", "verify", file];
  if (publicKey) {
    args.push("--key", publicKey);
  }
  return new Promise((resolve) => {
    execFile(command, args, { timeout: timeoutMs, windowsHide: true }, (error, stdout, stderr) => {
      const text = (stderr || stdout || "").trim();
      if (!error) {
        resolve({ status: "verified", message: text || "hash chain verified" });
        return;
      }
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "ENOENT") {
        resolve({ status: "unavailable", message: `${command} was not found; check PATH or the tpt-shell.cliPath setting` });
        return;
      }
      const exit = typeof code === "number" ? code : (error as { status?: number }).status;
      if (exit === 1) {
        resolve({ status: "broken", message: text || "verification failed" });
        return;
      }
      resolve({ status: "error", message: text || error.message });
    });
  });
}
