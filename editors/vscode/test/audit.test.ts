// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

import { test } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { parseAuditLines } from "../src/core/audit";
import { escapeHtml, renderAuditPage } from "../src/core/auditHtml";
import { runVerify } from "../src/core/verify";
import { runArgs } from "../src/core/runArgs";

const LOG = [
  JSON.stringify({
    seq: 0,
    wall_unix_ms: 1700000000000,
    actor: "ui",
    command: "notes.save",
    capability: "fs.write",
    outcome: "ok",
    error_code: null,
    duration_us: 1200,
  }),
  "",
  JSON.stringify({ seq: 1, wall_unix_ms: 1700000001000, actor: "agent", command: "x<script>", capability: null, outcome: "err", error_code: "IPC_DENIED" }),
  "{broken",
  "[1,2]",
].join("\n");

test("parses records and reports unreadable lines with their numbers", () => {
  const parsed = parseAuditLines(LOG);
  assert.equal(parsed.records.length, 2);
  assert.deepEqual(parsed.records[0], {
    line: 1,
    seq: 0,
    time: "2023-11-14T22:13:20.000Z",
    actor: "ui",
    command: "notes.save",
    capability: "fs.write",
    outcome: "ok",
    errorCode: null,
    durationUs: 1200,
  });
  assert.equal(parsed.records[1].errorCode, "IPC_DENIED");
  assert.deepEqual(
    parsed.problems.map((p) => p.line),
    [4, 5]
  );
});

test("escapes HTML in the audit page", () => {
  assert.equal(escapeHtml("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  const page = renderAuditPage("log.jsonl", parseAuditLines(LOG), { status: "broken", message: "line 2 <bad>" });
  assert.ok(!page.includes("<script>"));
  assert.ok(page.includes("x&lt;script&gt;"));
  assert.ok(page.includes("line 2 &lt;bad&gt;"));
  assert.match(page, /Content-Security-Policy" content="default-src 'none'/);
});

test("the verify status comes from the CLI exit code", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tpt-vscode-test-"));
  const file = path.join(dir, "log.jsonl");
  fs.writeFileSync(file, LOG);
  const node = process.execPath;
  // The stand-in program ignores its arguments and exits with the code it is given.
  const exitWith = (code: number) => ["-e", `process.exit(${code})`];
  assert.equal((await runVerify(node, exitWith(0), file, undefined)).status, "verified");
  assert.equal((await runVerify(node, exitWith(1), file, undefined)).status, "broken");
  assert.equal((await runVerify(node, exitWith(2), file, undefined)).status, "error");
  assert.equal((await runVerify("tpt-shell-does-not-exist-xyz", [], file, undefined)).status, "unavailable");
  fs.rmSync(dir, { recursive: true, force: true });
});

test("run arguments follow the CLI usage", () => {
  assert.deepEqual(runArgs({ command: "dev" }, "/app"), {
    ok: true,
    args: ["dev", "/app", "--port", "8080"],
    background: true,
  });
  assert.deepEqual(runArgs({ command: "dev", directory: "web", port: 3000 }, "/app"), {
    ok: true,
    args: ["dev", "web", "--port", "3000"],
    background: true,
  });
  assert.deepEqual(runArgs({ command: "build" }, "/app"), { ok: true, args: ["build", "/app"], background: false });
  assert.equal(runArgs({ command: "dev", port: 70000 }, "/app").ok, false);
  assert.equal(runArgs({ command: "test" }, "/app").ok, false);
});
