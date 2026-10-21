// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

import { test } from "node:test";
import * as assert from "node:assert/strict";
import * as fs from "node:fs";
import * as path from "node:path";
import { loadBundledData } from "../src/core/data";
import { checkManifest, completionsAt, hoverAt, tableAt } from "../src/core/manifest";

const root = path.join(__dirname, "..", "..");
const { schema, registry } = loadBundledData(root);
const repo = path.join(root, "..", "..");
const { parseRegistry } = require(path.join(root, "scripts", "sync-data.js")) as {
  parseRegistry: (rust: string) => { code: string }[];
};

const template = (name: string): string => fs.readFileSync(path.join(repo, "templates", name, "tpt-shell.toml"), "utf8");

function codes(text: string): string[] {
  return checkManifest(text, schema).map((d) => `${d.line} ${d.code}`);
}

test("the shipped templates pass the schema check", () => {
  for (const name of ["vanilla", "typescript", "appfront-dom"]) {
    assert.deepEqual(checkManifest(template(name), schema), [], name);
  }
});

test("an unknown key is TS0002 on its line", () => {
  assert.deepEqual(codes(["[app]", 'name = "x"', 'nmae = "typo"'].join("\n")), ["3 TS0002"]);
  assert.deepEqual(codes(["[app]", 'name = "x"', "[audit]", "store_parms = true"].join("\n")), ["4 TS0002"]);
});

test("a missing required key is TS0004", () => {
  assert.deepEqual(codes("[capabilities]\ngrant = []"), ["1 TS0004"]);
});

test("a value outside its rule is TS0003 and a wrong type is TS0001", () => {
  assert.deepEqual(codes(['[app]', 'name = "bad name!"'].join("\n")), ["2 TS0003"]);
  assert.deepEqual(codes(['[app]', 'name = "x"', "[[windows]]", 'name = "main"', "width = 0", "height = 10"].join("\n")), ["5 TS0003"]);
  assert.deepEqual(codes(['[app]', "name = 3"].join("\n")), ["2 TS0001"]);
  assert.deepEqual(codes('[app]\nname = "x"\n[audit]\nredact = "token"'), ["4 TS0001"]);
});

test("a syntax error is TS0001 and is reported with the schema problems", () => {
  const problems = codes('[app]\nname = \nnmae = "x"');
  assert.ok(problems.includes("2 TS0001"));
  assert.ok(problems.includes("3 TS0002"));
});

test("the table of a line comes from the nearest header above it", () => {
  const lines = ["[app]", 'name = "x"', "[[windows]]", 'name = "main"', "", "[audit]", "x = 1"];
  assert.deepEqual(tableAt(lines, 3), { segments: ["windows"], arrayTable: true });
  assert.deepEqual(tableAt(lines, 1), { segments: ["app"], arrayTable: false });
  assert.deepEqual(tableAt(["x = 1", "[app]"], 0), { segments: [], arrayTable: false });
});

test("key completion offers the table's keys that are not yet set", () => {
  const lines = ["[app]", 'name = "x"', "", "[[windows]]", "na"];
  const labels = completionsAt(lines, 4, 2, schema).map((c) => c.label);
  assert.deepEqual(labels.sort(), ["allow_navigation", "height", "name", "title", "width"]);
});

test("root completion offers section headers, and the header inserts its closing brackets", () => {
  const items = completionsAt(["["], 0, 1, schema);
  const audit = items.find((item) => item.label === "audit");
  assert.equal(audit?.insertText, "audit]");
  const windows = items.find((item) => item.label === "windows");
  assert.equal(windows?.insertText, "windows]]");
});

test("boolean values and enums are offered after the equals sign", () => {
  const lines = ["[audit]", "store_params = "];
  const values = completionsAt(lines, 1, 15, schema).map((c) => c.label);
  assert.deepEqual(values, ["true", "false"]);
});

test("hover shows the description and the rules of a key", () => {
  const lines = ["[audit]", "redact = []"];
  const hover = hoverAt(lines, 1, 2, schema);
  assert.ok(hover);
  assert.match(hover.markdown, /\*\*redact\*\* \(array of string\)/);
  assert.match(hover.markdown, /Object keys whose values are replaced/);
  assert.equal(hoverAt(lines, 1, 2 + 20, schema), undefined);
});

test("hover on a table header shows that section's description", () => {
  const hover = hoverAt(["[[windows]]"], 0, 4, schema);
  assert.ok(hover);
  assert.match(hover.markdown, /\*\*\[windows\]\*\*/);
});

test("the bundled registry matches the Rust source", () => {
  const rust = fs.readFileSync(path.join(repo, "crates", "tpt-shell-errors", "src", "lib.rs"), "utf8");
  const fromSource = parseRegistry(rust);
  assert.equal(registry.size, fromSource.length);
  for (const entry of fromSource) {
    assert.deepEqual(registry.lookup(entry.code), entry, entry.code);
  }
  assert.equal(registry.lookup("ts0002")?.code, "TS0002");
  assert.equal(registry.lookup("TS9999"), undefined);
});

test("the bundled schema is the published one", () => {
  const published = fs.readFileSync(path.join(repo, "crates", "tpt-shell-manifest", "schema", "tpt-shell.manifest.schema.json"), "utf8");
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, "data", "tpt-shell.manifest.schema.json"), "utf8")), JSON.parse(published));
});
