// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

import { test } from "node:test";
import * as assert from "node:assert/strict";
import { callJson, ContractCommand, exampleArgs, fieldsOf, parseContract } from "../src/core/contract";
import { codeAt, findCodes, Registry, explainMarkdown } from "../src/core/registry";

const SAMPLE = JSON.stringify({
  contract_version: 1,
  commands: [
    {
      name: "notes.save",
      kind: "async",
      capability: "fs.write",
      args: {
        type: "object",
        properties: {
          title: { type: "string", maxLength: 64, pattern: "^[a-z]+$" },
          count: { type: "integer", minimum: 0, maximum: 9 },
          tags: { type: "array", items: { type: "string", maxLength: 8 }, maxItems: 4 },
          mode: { enum: ["fast", "slow"], type: "string" },
          note: { anyOf: [{ type: "string", maxLength: 10 }, { type: "null" }] },
        },
        required: ["title", "count", "tags", "mode"],
        additionalProperties: false,
      },
      flags: { agent: true, ui: false },
    },
    {
      name: "app.ping",
      kind: "sync",
      capability: null,
      args: { type: "object", properties: {}, required: [], additionalProperties: false },
      flags: { agent: false, ui: true },
    },
  ],
});

test("parses commands with kind, capability and flags", () => {
  const parsed = parseContract(SAMPLE);
  assert.equal(parsed.ok, true);
  if (!parsed.ok) {
    return;
  }
  assert.equal(parsed.contract.version, 1);
  const [save, ping] = parsed.contract.commands;
  assert.equal(save.name, "notes.save");
  assert.equal(save.kind, "async");
  assert.equal(save.capability, "fs.write");
  assert.equal(save.agent, true);
  assert.equal(save.ui, false);
  assert.equal(ping.capability, null);
  assert.equal(ping.ui, true);
});

test("lists the fields with type, requiredness and constraints", () => {
  const parsed = parseContract(SAMPLE);
  assert.equal(parsed.ok, true);
  if (!parsed.ok) {
    return;
  }
  const fields = fieldsOf(parsed.contract.commands[0].args);
  const byName = new Map(fields.map((f) => [f.name, f]));
  assert.equal(byName.get("title")?.type, "string");
  assert.equal(byName.get("title")?.required, true);
  assert.deepEqual(byName.get("title")?.constraints, ["length 0 to 64", "pattern ^[a-z]+$"]);
  assert.equal(byName.get("count")?.type, "int");
  assert.deepEqual(byName.get("count")?.constraints, ["range 0 to 9"]);
  assert.equal(byName.get("tags")?.type, "list<string>");
  assert.equal(byName.get("note")?.required, false);
  assert.equal(byName.get("note")?.type, "string");
  assert.equal(byName.get("mode")?.type, 'one of "fast" | "slow"');
});

test("the call JSON has the command name and only the required arguments", () => {
  const parsed = parseContract(SAMPLE);
  assert.equal(parsed.ok, true);
  if (!parsed.ok) {
    return;
  }
  const text = callJson(parsed.contract.commands[0] as ContractCommand);
  assert.deepEqual(JSON.parse(text), {
    command: "notes.save",
    args: { title: "", count: 0, tags: [], mode: "fast" },
  });
  assert.deepEqual(exampleArgs({ type: "object", properties: {} }), {});
});

test("a file that is not a contract is refused with a reason", () => {
  const broken = parseContract("{not json");
  assert.equal(broken.ok, false);
  assert.match(broken.ok ? "" : broken.message, /not valid JSON/);
  const noCommands = parseContract('{"contract_version":1}');
  assert.equal(noCommands.ok, false);
  const noVersion = parseContract('{"commands":[]}');
  assert.equal(noVersion.ok, false);
});

test("finds TS codes in a line and looks them up case-insensitively", () => {
  assert.deepEqual(
    findCodes("error TS0002 and ts0003").map((m) => m.code),
    ["TS0002"]
  );
  assert.equal(codeAt("x TS0100 y", 3)?.code, "TS0100");
  assert.equal(codeAt("x TS0100 y", 0), undefined);
  const registry = new Registry([{ code: "TS0100", title: "t", explanation: "e", fix: "f" }]);
  assert.equal(registry.lookup(" ts0100 ")?.title, "t");
  assert.equal(explainMarkdown(registry.lookup("TS0100")!), "**TS0100: t**\n\ne\n\nFix: f");
});
