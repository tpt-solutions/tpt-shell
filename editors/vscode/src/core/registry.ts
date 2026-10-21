// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The stable TS#### error registry, as the CLI's `tpt-shell explain` prints it. The entries come
// from data/registry.json, which the sync script copies from the Rust source.

export interface RegistryEntry {
  code: string;
  title: string;
  explanation: string;
  fix: string;
}

export interface CodeMatch {
  code: string;
  start: number;
  end: number;
}

export class Registry {
  private readonly byCode: Map<string, RegistryEntry>;

  constructor(entries: RegistryEntry[]) {
    this.byCode = new Map(entries.map((entry) => [entry.code.toUpperCase(), entry]));
  }

  // Case-insensitive, like the CLI.
  lookup(code: string): RegistryEntry | undefined {
    return this.byCode.get(code.trim().toUpperCase());
  }

  get size(): number {
    return this.byCode.size;
  }
}

// Every TS#### code in one line of text, with its column span.
export function findCodes(text: string): CodeMatch[] {
  const matches: CodeMatch[] = [];
  const pattern = /\bTS\d{4}\b/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(text)) !== null) {
    matches.push({ code: match[0], start: match.index, end: match.index + match[0].length });
  }
  return matches;
}

// The code under a column, if any.
export function codeAt(text: string, character: number): CodeMatch | undefined {
  return findCodes(text).find((match) => character >= match.start && character <= match.end);
}

// Markdown for a hover, in the same shape as `tpt-shell explain`.
export function explainMarkdown(entry: RegistryEntry): string {
  return [`**${entry.code}: ${entry.title}**`, "", entry.explanation, "", `Fix: ${entry.fix}`].join("\n");
}
