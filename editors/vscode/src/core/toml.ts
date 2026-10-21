// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// A TOML 1.0 reader for tpt-shell.toml. It builds the plain value tree, records the line of
// every key and table, and reports syntax problems as TS0001. It is not a full TOML library:
// dates are kept as their text, and the schema check decides what a value may be.

export interface TomlIssue {
  line: number;
  code: "TS0001";
  message: string;
}

export interface TomlDocument {
  data: Record<string, unknown>;
  // Line (1-based) of each key and table. A key is named by its pointer: segments joined with
  // "/", and array elements by their index, for example "/windows/0/width".
  lines: Map<string, number>;
  issues: TomlIssue[];
}

class SyntaxProblem extends Error {
  constructor(message: string, readonly pos: number) {
    super(message);
  }
}

const BARE_KEY = /^[A-Za-z0-9_-]+/;
const NUMBER = /^[+-]?(?:\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d[\d_]*)?|0x[0-9A-Fa-f_]+|0o[0-7_]+|0b[01_]+|inf|nan)$/;
const DATE = /^\d{4}-\d{2}-\d{2}/;

export function parseToml(text: string): TomlDocument {
  const scanner = new Scanner(text);
  const data: Record<string, unknown> = {};
  const lines = new Map<string, number>();
  const issues: TomlIssue[] = [];
  const definedTables = new Set<string>();
  let table: Record<string, unknown> = data;
  let tablePointer = "";

  for (;;) {
    scanner.skipBlank();
    if (scanner.atEnd()) {
      break;
    }
    const start = scanner.pos;
    try {
      if (scanner.peek() === "[") {
        const isArray = scanner.text[scanner.pos + 1] === "[";
        scanner.pos += isArray ? 2 : 1;
        const segments = parseKeyPath(scanner);
        scanner.skipSpaces();
        scanner.expect(isArray ? "]]" : "]");
        const pointer = pointerOf(segments);
        const line = scanner.lineAt(start);
        if (isArray) {
          const parent = navigate(data, segments.slice(0, -1), true, start);
          const last = segments[segments.length - 1];
          const existing = parent[last];
          const list = existing === undefined ? [] : existing;
          if (!Array.isArray(list)) {
            throw new SyntaxProblem(`\`${segments.join(".")}\` is already a table`, start);
          }
          parent[last] = list;
          const element: Record<string, unknown> = {};
          list.push(element);
          table = element;
          tablePointer = `${pointer}/${list.length - 1}`;
        } else {
          if (definedTables.has(pointer)) {
            throw new SyntaxProblem(`table \`${segments.join(".")}\` is defined twice`, start);
          }
          definedTables.add(pointer);
          table = navigate(data, segments, true, start);
          tablePointer = pointer;
        }
        lines.set(tablePointer, line);
      } else {
        const keys = parseKeyPath(scanner);
        scanner.skipSpaces();
        scanner.expect("=");
        scanner.skipSpaces();
        const keyPointer = joinPointer(tablePointer, keys);
        const value = parseValue(scanner, keyPointer, lines);
        const parent = navigate(table, keys.slice(0, -1), true, start);
        const last = keys[keys.length - 1];
        if (Object.prototype.hasOwnProperty.call(parent, last)) {
          throw new SyntaxProblem(`key \`${keys.join(".")}\` is defined twice`, start);
        }
        parent[last] = value;
        lines.set(keyPointer, scanner.lineAt(start));
      }
      scanner.expectLineEnd();
    } catch (error) {
      if (!(error instanceof SyntaxProblem)) {
        throw error;
      }
      issues.push({ line: scanner.lineAt(error.pos), code: "TS0001", message: error.message });
      scanner.skipToLineEnd();
    }
  }
  return { data, lines, issues };
}

// Pointer of a table or key from its path, with the table's own pointer as the prefix.
export function pointerOf(segments: string[]): string {
  return segments.map((segment) => "/" + segment).join("");
}

function joinPointer(prefix: string, segments: string[]): string {
  return prefix + pointerOf(segments);
}

// Walks down the dotted path, creating tables when asked. An array of tables means its last
// element, which is how a `[[windows]]` table is addressed by a later `[windows.x]` header.
function navigate(root: Record<string, unknown>, segments: string[], create: boolean, pos: number): Record<string, unknown> {
  let node = root;
  for (const segment of segments) {
    let next = node[segment];
    if (next === undefined && create) {
      next = {};
      node[segment] = next;
    }
    if (Array.isArray(next)) {
      next = next[next.length - 1];
    }
    if (next === null || typeof next !== "object") {
      throw new SyntaxProblem(`\`${segment}\` is not a table`, pos);
    }
    node = next as Record<string, unknown>;
  }
  return node;
}

function parseKeyPath(scanner: Scanner): string[] {
  const segments: string[] = [];
  for (;;) {
    scanner.skipSpaces();
    const quote = scanner.peek();
    let key: string | undefined;
    if (quote === '"') {
      key = parseBasicString(scanner);
    } else if (quote === "'") {
      key = parseLiteralString(scanner);
    } else {
      const match = BARE_KEY.exec(scanner.text.slice(scanner.pos, scanner.pos + 256));
      if (match) {
        key = match[0];
        scanner.pos += key.length;
      }
    }
    if (key === undefined || key === "") {
      throw new SyntaxProblem("expected a key", scanner.pos);
    }
    segments.push(key);
    scanner.skipSpaces();
    if (scanner.peek() !== ".") {
      return segments;
    }
    scanner.pos += 1;
  }
}

function parseValue(scanner: Scanner, pointer: string, lines: Map<string, number>): unknown {
  const c = scanner.peek();
  if (c === '"') {
    if (scanner.startsWith('"""')) {
      return parseMultiline(scanner, '"""', true);
    }
    return parseBasicString(scanner);
  }
  if (c === "'") {
    if (scanner.startsWith("'''")) {
      return parseMultiline(scanner, "'''", false);
    }
    return parseLiteralString(scanner);
  }
  if (c === "[") {
    scanner.pos += 1;
    const list: unknown[] = [];
    for (;;) {
      scanner.skipBlank();
      if (scanner.peek() === "]") {
        scanner.pos += 1;
        return list;
      }
      list.push(parseValue(scanner, `${pointer}/${list.length}`, lines));
      scanner.skipBlank();
      if (scanner.peek() === ",") {
        scanner.pos += 1;
        continue;
      }
      scanner.expect("]");
      return list;
    }
  }
  if (c === "{") {
    scanner.pos += 1;
    const table: Record<string, unknown> = {};
    scanner.skipSpaces();
    if (scanner.peek() === "}") {
      scanner.pos += 1;
      return table;
    }
    for (;;) {
      const keyStart = scanner.pos;
      const keys = parseKeyPath(scanner);
      scanner.skipSpaces();
      scanner.expect("=");
      scanner.skipSpaces();
      const keyPointer = joinPointer(pointer, keys);
      const value = parseValue(scanner, keyPointer, lines);
      const parent = navigate(table, keys.slice(0, -1), true, keyStart);
      parent[keys[keys.length - 1]] = value;
      lines.set(keyPointer, scanner.lineAt(keyStart));
      scanner.skipSpaces();
      if (scanner.peek() === ",") {
        scanner.pos += 1;
        scanner.skipSpaces();
        continue;
      }
      scanner.expect("}");
      return table;
    }
  }
  const token = /^[^\s,\]}#]+/.exec(scanner.text.slice(scanner.pos, scanner.pos + 512));
  if (!token) {
    throw new SyntaxProblem("expected a value", scanner.pos);
  }
  const raw = token[0];
  scanner.pos += raw.length;
  if (raw === "true" || raw === "false") {
    return raw === "true";
  }
  if (/^[+-]?(inf|nan)$/.test(raw)) {
    return raw.endsWith("nan") ? NaN : raw.startsWith("-") ? -Infinity : Infinity;
  }
  if (NUMBER.test(raw)) {
    return Number(raw.replace(/_/g, ""));
  }
  if (DATE.test(raw)) {
    return raw;
  }
  throw new SyntaxProblem(`invalid value \`${raw}\``, scanner.pos - raw.length);
}

const ESCAPES: Record<string, string> = {
  b: "\b",
  t: "\t",
  n: "\n",
  f: "\f",
  r: "\r",
  '"': '"',
  "\\": "\\",
};

function parseBasicString(scanner: Scanner): string {
  scanner.expect('"');
  const start = scanner.pos;
  for (;;) {
    const c = scanner.text[scanner.pos];
    if (c === undefined || c === "\n" || c === "\r") {
      throw new SyntaxProblem("unterminated string", start);
    }
    if (c === '"') {
      const raw = scanner.text.slice(start, scanner.pos);
      scanner.pos += 1;
      return decodeEscapes(raw, start);
    }
    scanner.pos += c === "\\" ? 2 : 1;
  }
}

// Decodes the escapes of a basic string body. `offset` is where the body starts in the document,
// for error positions.
function decodeEscapes(raw: string, offset: number): string {
  let out = "";
  for (let i = 0; i < raw.length; i++) {
    const c = raw[i];
    if (c !== "\\") {
      out += c;
      continue;
    }
    const e = raw[i + 1] ?? "";
    i += 1;
    if (e in ESCAPES) {
      out += ESCAPES[e];
    } else if (e === "u" || e === "U") {
      const width = e === "u" ? 4 : 8;
      const hex = raw.slice(i + 1, i + 1 + width);
      if (hex.length !== width || !/^[0-9A-Fa-f]+$/.test(hex)) {
        throw new SyntaxProblem("invalid unicode escape", offset + i);
      }
      out += String.fromCodePoint(parseInt(hex, 16));
      i += width;
    } else {
      throw new SyntaxProblem(`invalid escape \`\\${e}\``, offset + i - 1);
    }
  }
  return out;
}

function parseLiteralString(scanner: Scanner): string {
  scanner.expect("'");
  const end = scanner.text.indexOf("'", scanner.pos);
  const newline = scanner.text.slice(scanner.pos).search(/[\r\n]/);
  if (end < 0 || (newline >= 0 && newline < end - scanner.pos)) {
    throw new SyntaxProblem("unterminated string", scanner.pos);
  }
  const value = scanner.text.slice(scanner.pos, end);
  scanner.pos = end + 1;
  return value;
}

// Triple-quoted strings. A line ending right after the opening quotes is dropped, as TOML says.
function parseMultiline(scanner: Scanner, delimiter: string, escapes: boolean): string {
  scanner.pos += 3;
  if (scanner.text.startsWith("\r\n", scanner.pos)) {
    scanner.pos += 2;
  } else if (scanner.text[scanner.pos] === "\n") {
    scanner.pos += 1;
  }
  const end = scanner.text.indexOf(delimiter, scanner.pos);
  if (end < 0) {
    throw new SyntaxProblem("unterminated multi-line string", scanner.pos);
  }
  const raw = scanner.text.slice(scanner.pos, end);
  scanner.pos = end + 3;
  if (!escapes) {
    return raw;
  }
  // A backslash at the end of a line joins it with the next line, with the spaces that follow.
  return decodeEscapes(raw.replace(/\\[ \t]*\r?\n\s*/g, ""), scanner.pos - raw.length);
}

class Scanner {
  pos = 0;
  private readonly newlines: number[] = [];

  constructor(readonly text: string) {
    for (let i = 0; i < text.length; i++) {
      if (text[i] === "\n") {
        this.newlines.push(i);
      }
    }
  }

  atEnd(): boolean {
    return this.pos >= this.text.length;
  }

  peek(): string {
    return this.text[this.pos] ?? "";
  }

  startsWith(prefix: string): boolean {
    return this.text.startsWith(prefix, this.pos);
  }

  expect(prefix: string): void {
    if (!this.text.startsWith(prefix, this.pos)) {
      throw new SyntaxProblem(`expected \`${prefix}\``, this.pos);
    }
    this.pos += prefix.length;
  }

  skipSpaces(): void {
    while (this.peek() === " " || this.peek() === "\t") {
      this.pos += 1;
    }
  }

  // Spaces, line breaks and comments.
  skipBlank(): void {
    for (;;) {
      const c = this.peek();
      if (c === " " || c === "\t" || c === "\r" || c === "\n") {
        this.pos += 1;
      } else if (c === "#") {
        this.skipToLineEnd();
      } else {
        return;
      }
    }
  }

  expectLineEnd(): void {
    this.skipSpaces();
    if (this.peek() === "#") {
      this.skipToLineEnd();
    }
    const c = this.peek();
    if (c !== "" && c !== "\n" && c !== "\r") {
      throw new SyntaxProblem("unexpected text after the value", this.pos);
    }
  }

  skipToLineEnd(): void {
    const next = this.text.indexOf("\n", this.pos);
    this.pos = next < 0 ? this.text.length : next;
  }

  // 1-based line of an offset: the number of line breaks before it, plus one.
  lineAt(offset: number): number {
    let low = 0;
    let high = this.newlines.length;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (this.newlines[mid] < offset) {
        low = mid + 1;
      } else {
        high = mid;
      }
    }
    return low + 1;
  }
}
