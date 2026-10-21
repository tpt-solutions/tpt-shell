// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The manifest features that need no editor API: checks, completion and hover for tpt-shell.toml,
// all driven by the published JSON Schema. The VS Code layer maps these results to editor types.

import { parseToml } from "./toml";
import { SchemaNode, validate } from "./schema";

export interface ManifestDiagnostic {
  line: number; // 1-based
  code: string;
  message: string;
}

export function checkManifest(text: string, schema: SchemaNode): ManifestDiagnostic[] {
  const doc = parseToml(text);
  const diagnostics: ManifestDiagnostic[] = doc.issues.map((issue) => ({
    line: issue.line,
    code: issue.code,
    message: issue.message,
  }));
  for (const problem of validate(doc.data, schema)) {
    diagnostics.push({
      line: lineFor(problem.pointer, doc.lines),
      code: problem.code,
      message: problem.message,
    });
  }
  return diagnostics.sort((a, b) => a.line - b.line);
}

// The line of a pointer, or of its nearest recorded ancestor. Line 1 when nothing is recorded.
function lineFor(pointer: string, lines: Map<string, number>): number {
  let current = pointer;
  for (;;) {
    const line = lines.get(current);
    if (line !== undefined) {
      return line;
    }
    const cut = current.lastIndexOf("/");
    if (cut <= 0) {
      return lines.get("") ?? 1;
    }
    current = current.slice(0, cut);
  }
}

export interface TableContext {
  segments: string[];
  arrayTable: boolean;
}

const HEADER = /^\s*(\[\[?)\s*([^\]]*?)\s*\]\]?\s*(#.*)?$/;

// The table that a line sits in: the last header above it, or the root when there is none.
export function tableAt(lines: string[], line: number): TableContext {
  let context: TableContext = { segments: [], arrayTable: false };
  for (let i = 0; i <= line && i < lines.length; i++) {
    const match = HEADER.exec(lines[i]);
    if (match) {
      context = {
        segments: match[2].split(".").map((part) => part.trim().replace(/^["']|["']$/g, "")),
        arrayTable: match[1] === "[[",
      };
    }
  }
  return context;
}

// The schema node for a table. An array of tables is described by the schema of its items.
export function schemaFor(root: SchemaNode, context: TableContext): SchemaNode | undefined {
  let node: SchemaNode | undefined = root;
  for (const segment of context.segments) {
    node = node?.properties?.[segment];
    if (node?.type === "array" && node.items) {
      node = node.items;
    }
  }
  return node;
}

export type CompletionKind = "table" | "key" | "value";

export interface ManifestCompletion {
  label: string;
  kind: CompletionKind;
  detail: string;
  documentation: string;
  insertText: string;
}

const KEY_PREFIX = /^\s*[A-Za-z0-9_-]*$/;
const HEADER_PREFIX = /^\s*\[\[?\s*[A-Za-z0-9_.-]*$/;
const VALUE_PREFIX = /^\s*([A-Za-z0-9_-]+)\s*=\s*\S*$/;

// Completions at a position. The text before the cursor decides which list is offered.
export function completionsAt(lines: string[], line: number, character: number, root: SchemaNode): ManifestCompletion[] {
  const before = (lines[line] ?? "").slice(0, character);
  const context = tableAt(lines, line);
  if (HEADER_PREFIX.test(before)) {
    return Object.entries(root.properties ?? {})
      .filter(([, node]) => isTable(node))
      .map(([name, node]) => describe(name, node, "table", name + (node.type === "array" ? "]]" : "]")));
  }
  if (KEY_PREFIX.test(before)) {
    const node = schemaFor(root, context);
    if (!node?.properties) {
      return [];
    }
    const taken = keysInTable(lines, line);
    return Object.entries(node.properties)
      .filter(([name, child]) => !taken.has(name) && !isTable(child))
      .map(([name, child]) => describe(name, child, "key", `${name} = `));
  }
  const value = VALUE_PREFIX.exec(before);
  if (value) {
    const node = schemaFor(root, context)?.properties?.[value[1]];
    if (!node) {
      return [];
    }
    const options: unknown[] = node.enum ?? (node.type === "boolean" ? [true, false] : []);
    return options.map((option) => ({
      label: String(option),
      kind: "value" as const,
      detail: node.type ?? "",
      documentation: node.description ?? "",
      insertText: String(option),
    }));
  }
  return [];
}

// A section: an object, or an array of tables. Other arrays are values.
function isTable(node: SchemaNode): boolean {
  return node.type === "object" || (node.type === "array" && node.items?.type === "object");
}

function describe(name: string, node: SchemaNode, kind: CompletionKind, insertText: string): ManifestCompletion {
  return {
    label: name,
    kind,
    detail: typeLabel(node),
    documentation: node.description ?? "",
    insertText,
  };
}

// Keys already set in the table the line belongs to, so they are not offered again.
function keysInTable(lines: string[], line: number): Set<string> {
  let start = -1;
  for (let i = line; i >= 0; i--) {
    if (HEADER.test(lines[i] ?? "")) {
      start = i;
      break;
    }
  }
  const taken = new Set<string>();
  for (let i = start + 1; i < lines.length; i++) {
    if (HEADER.test(lines[i])) {
      break;
    }
    const match = /^s*([A-Za-z0-9_-]+)s*=/.exec(lines[i]);
    if (match) {
      taken.add(match[1]);
    }
  }
  return taken;
}

export interface ManifestHover {
  markdown: string;
}

// Hover for a key or a table name. Returns undefined when the word is not in the schema.
export function hoverAt(lines: string[], line: number, character: number, root: SchemaNode): ManifestHover | undefined {
  const text = lines[line] ?? "";
  const word = wordAt(text, character);
  if (!word) {
    return undefined;
  }
  const header = HEADER.exec(text);
  if (header) {
    const node = root.properties?.[header[2].split(".")[0]];
    if (!node) {
      return undefined;
    }
    return { markdown: markdownFor(`[${header[2]}]`, node) };
  }
  const context = tableAt(lines, line);
  const node = schemaFor(root, context)?.properties?.[word];
  if (!node || !/^\s*[A-Za-z0-9_-]+\s*=/.test(text)) {
    return undefined;
  }
  return { markdown: markdownFor(word, node) };
}

function wordAt(text: string, character: number): string | undefined {
  const pattern = /[A-Za-z0-9_-]+/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(text)) !== null) {
    if (character >= match.index && character <= match.index + match[0].length) {
      return match[0];
    }
  }
  return undefined;
}

function markdownFor(name: string, node: SchemaNode): string {
  const lines = [`**${name}** (${typeLabel(node)})`];
  if (node.description) {
    lines.push("", node.description);
  }
  const rules: string[] = [];
  if (node.default !== undefined) {
    rules.push(`default: ${JSON.stringify(node.default)}`);
  }
  if (node.enum) {
    rules.push(`one of: ${node.enum.map((o) => JSON.stringify(o)).join(", ")}`);
  }
  if (node.pattern) {
    rules.push(`pattern: \`${node.pattern}\``);
  }
  if (node.minLength !== undefined || node.maxLength !== undefined) {
    rules.push(`length: ${node.minLength ?? 0} to ${node.maxLength ?? "any"}`);
  }
  if (node.minimum !== undefined || node.exclusiveMinimum !== undefined) {
    rules.push(node.exclusiveMinimum !== undefined ? `greater than ${node.exclusiveMinimum}` : `at least ${node.minimum}`);
  }
  if (rules.length > 0) {
    lines.push("", rules.map((rule) => `- ${rule}`).join("\n"));
  }
  return lines.join("\n");
}

function typeLabel(node: SchemaNode): string {
  if (node.type === "array" && node.items) {
    return `array of ${typeLabel(node.items)}`;
  }
  return node.type ?? "any";
}
