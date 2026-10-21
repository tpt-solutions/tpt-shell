// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Copies the manifest JSON Schema and the error registry from the Rust sources into data/, so
// the extension ships the same text the CLI uses. Run by `npm run sync` (and so by `compile`).

const fs = require("fs");
const path = require("path");

const root = path.join(__dirname, "..", "..", "..");
const schemaSource = path.join(root, "crates", "tpt-shell-manifest", "schema", "tpt-shell.manifest.schema.json");
const registrySource = path.join(root, "crates", "tpt-shell-errors", "src", "lib.rs");
const dataDir = path.join(__dirname, "..", "data");

// One Rust string literal: any run of characters that are not quote or backslash, or an escape.
const STRING = "((?:[^\"\\\\]|\\\\.)*)";

// Reads the REGISTRY literal from tpt-shell-errors. Each entry has the same four fields in the
// same order, so one pattern covers them all.
function parseRegistry(rust) {
  const entry = new RegExp(
    "Entry\\s*\\{\\s*code:\\s*\"([^\"]*)\",\\s*title:\\s*\"" + STRING +
      "\",\\s*explanation:\\s*\"" + STRING + "\",\\s*fix:\\s*\"" + STRING + "\",\\s*\\}",
    "g"
  );
  const unescape = (text) => JSON.parse("\"" + text + "\"");
  const entries = [];
  let match;
  while ((match = entry.exec(rust)) !== null) {
    entries.push({
      code: match[1],
      title: unescape(match[2]),
      explanation: unescape(match[3]),
      fix: unescape(match[4]),
    });
  }
  if (entries.length === 0) {
    throw new Error("no registry entries found in " + registrySource);
  }
  return entries;
}

function main() {
  fs.mkdirSync(dataDir, { recursive: true });
  fs.copyFileSync(schemaSource, path.join(dataDir, "tpt-shell.manifest.schema.json"));
  const entries = parseRegistry(fs.readFileSync(registrySource, "utf8"));
  fs.writeFileSync(path.join(dataDir, "registry.json"), JSON.stringify(entries, null, 2) + "\n");
  console.log("synced " + entries.length + " error codes and the manifest schema into data/");
}

if (require.main === module) {
  main();
}

module.exports = { parseRegistry };
