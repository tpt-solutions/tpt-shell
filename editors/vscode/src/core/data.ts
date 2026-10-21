// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Loads the two bundled data files: the published manifest schema and the error registry.

import * as fs from "fs";
import * as path from "path";
import { Registry, RegistryEntry } from "./registry";
import { SchemaNode } from "./schema";

export interface BundledData {
  schema: SchemaNode;
  registry: Registry;
}

// `extensionRoot` is the folder that holds package.json. The files live in its data/ folder.
export function loadBundledData(extensionRoot: string): BundledData {
  const dir = path.join(extensionRoot, "data");
  const schema = JSON.parse(fs.readFileSync(path.join(dir, "tpt-shell.manifest.schema.json"), "utf8")) as SchemaNode;
  const entries = JSON.parse(fs.readFileSync(path.join(dir, "registry.json"), "utf8")) as RegistryEntry[];
  return { schema, registry: new Registry(entries) };
}
