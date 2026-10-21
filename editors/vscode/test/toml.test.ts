// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

import { test } from "node:test";
import * as assert from "node:assert/strict";
import { parseToml } from "../src/core/toml";

test("reads tables, array tables and values", () => {
  const doc = parseToml(
    [
      "# comment",
      "[app]",
      'name = "my-app"',
      "",
      "[[windows]]",
      'name = "main"',
      "width = 960",
      "[[windows]]",
      'name = "side"',
      "height = 1_200.5",
      "[capabilities]",
      "grant = [",
      '  "fs.read",',
      '  "net.fetch",',
      "]",
    ].join("\n")
  );
  assert.deepEqual(doc.issues, []);
  assert.deepEqual(doc.data, {
    app: { name: "my-app" },
    windows: [{ name: "main", width: 960 }, { name: "side", height: 1200.5 }],
    capabilities: { grant: ["fs.read", "net.fetch"] },
  });
});

test("records the line of each key and table", () => {
  const doc = parseToml(["[app]", 'name = "x"', "[[windows]]", "width = 1"].join("\n"));
  assert.equal(doc.lines.get("/app"), 1);
  assert.equal(doc.lines.get("/app/name"), 2);
  assert.equal(doc.lines.get("/windows/0"), 3);
  assert.equal(doc.lines.get("/windows/0/width"), 4);
});

test("reports a syntax problem as TS0001 on its line and keeps reading", () => {
  const doc = parseToml(["[app]", "name = ", 'version = "1.0"'].join("\n"));
  assert.equal(doc.issues.length, 1);
  assert.equal(doc.issues[0].code, "TS0001");
  assert.equal(doc.issues[0].line, 2);
  assert.equal((doc.data.app as Record<string, unknown>).version, "1.0");
});

test("reports a key defined twice and a table defined twice", () => {
  const doc = parseToml(["[app]", 'name = "a"', 'name = "b"', "[app]"].join("\n"));
  assert.deepEqual(
    doc.issues.map((issue) => issue.line),
    [3, 4]
  );
});

test("handles escapes, literal strings, inline tables and comments after values", () => {
  const doc = parseToml(
    [
      "[t]",
      'a = "tab\\there \\u0041"',
      "b = 'C:\\raw'",
      "c = { x = 1, y = \"two\" }",
      "d = true # trailing comment",
      'e = """multi',
      'line"""',
    ].join("\n")
  );
  assert.deepEqual(doc.issues, []);
  assert.deepEqual(doc.data.t, {
    a: "tab\there A",
    b: "C:\\raw",
    c: { x: 1, y: "two" },
    d: true,
    e: "multi\nline",
  });
  assert.equal(doc.lines.get("/t/c/y"), 4);
});

test("accepts dates as text and rejects bare words", () => {
  const ok = parseToml("[x]\nwhen = 1979-05-27T07:32:00Z");
  assert.deepEqual(ok.issues, []);
  assert.equal(ok.data.x && (ok.data.x as Record<string, unknown>).when, "1979-05-27T07:32:00Z");
  const bad = parseToml("[x]\nname = hello");
  assert.equal(bad.issues[0]?.code, "TS0001");
  assert.equal(bad.issues[0]?.line, 2);
});
