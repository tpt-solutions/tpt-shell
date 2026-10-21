// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// A checker for the subset of JSON Schema that tpt-shell.manifest.schema.json uses: type,
// properties, required, additionalProperties, items, pattern, length, bounds and enum. Each
// problem is reported under the code the CLI uses for it (TS0001 to TS0004).

export interface SchemaNode {
  type?: string;
  description?: string;
  default?: unknown;
  properties?: Record<string, SchemaNode>;
  required?: string[];
  additionalProperties?: boolean | SchemaNode;
  items?: SchemaNode;
  pattern?: string;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  exclusiveMinimum?: number;
  exclusiveMaximum?: number;
  enum?: unknown[];
}

export type ProblemCode = "TS0001" | "TS0002" | "TS0003" | "TS0004";

export interface SchemaProblem {
  // Pointer of the value or key the problem is about, as in the TOML reader's line map.
  pointer: string;
  code: ProblemCode;
  message: string;
}

export function validate(value: unknown, schema: SchemaNode, pointer = ""): SchemaProblem[] {
  const problems: SchemaProblem[] = [];
  check(value, schema, pointer, problems);
  return problems;
}

function check(value: unknown, schema: SchemaNode, pointer: string, out: SchemaProblem[]): void {
  if (schema.type !== undefined && !matchesType(value, schema.type)) {
    out.push({ pointer, code: "TS0001", message: `expected ${article(schema.type)} ${schema.type}, found ${describe(value)}` });
    return;
  }
  if (isPlainObject(value)) {
    checkObject(value, schema, pointer, out);
  } else if (Array.isArray(value)) {
    if (schema.items) {
      value.forEach((item, index) => check(item, schema.items as SchemaNode, `${pointer}/${index}`, out));
    }
  } else if (typeof value === "string") {
    checkString(value, schema, pointer, out);
  } else if (typeof value === "number") {
    checkNumber(value, schema, pointer, out);
  }
  if (schema.enum && !schema.enum.some((option) => option === value)) {
    out.push({ pointer, code: "TS0003", message: `must be one of ${schema.enum.map((o) => JSON.stringify(o)).join(", ")}` });
  }
}

function checkObject(value: Record<string, unknown>, schema: SchemaNode, pointer: string, out: SchemaProblem[]): void {
  for (const key of schema.required ?? []) {
    if (!Object.prototype.hasOwnProperty.call(value, key)) {
      out.push({ pointer, code: "TS0004", message: `missing key \`${key}\`` });
    }
  }
  const properties = schema.properties ?? {};
  for (const key of Object.keys(value)) {
    const childPointer = `${pointer}/${key}`;
    if (Object.prototype.hasOwnProperty.call(properties, key)) {
      check(value[key], properties[key], childPointer, out);
    } else if (schema.additionalProperties === false) {
      out.push({ pointer: childPointer, code: "TS0002", message: `unknown key \`${key}\`` });
    } else if (typeof schema.additionalProperties === "object") {
      check(value[key], schema.additionalProperties, childPointer, out);
    }
  }
}

function checkString(value: string, schema: SchemaNode, pointer: string, out: SchemaProblem[]): void {
  const length = [...value].length;
  if (schema.minLength !== undefined && length < schema.minLength) {
    out.push({ pointer, code: "TS0003", message: `must be at least ${schema.minLength} characters` });
  }
  if (schema.maxLength !== undefined && length > schema.maxLength) {
    out.push({ pointer, code: "TS0003", message: `must be at most ${schema.maxLength} characters` });
  }
  if (schema.pattern !== undefined && !new RegExp(schema.pattern, "u").test(value)) {
    out.push({ pointer, code: "TS0003", message: `does not match the pattern ${schema.pattern}` });
  }
}

function checkNumber(value: number, schema: SchemaNode, pointer: string, out: SchemaProblem[]): void {
  if (schema.minimum !== undefined && value < schema.minimum) {
    out.push({ pointer, code: "TS0003", message: `must be at least ${schema.minimum}` });
  }
  if (schema.maximum !== undefined && value > schema.maximum) {
    out.push({ pointer, code: "TS0003", message: `must be at most ${schema.maximum}` });
  }
  if (schema.exclusiveMinimum !== undefined && value <= schema.exclusiveMinimum) {
    out.push({ pointer, code: "TS0003", message: `must be greater than ${schema.exclusiveMinimum}` });
  }
  if (schema.exclusiveMaximum !== undefined && value >= schema.exclusiveMaximum) {
    out.push({ pointer, code: "TS0003", message: `must be less than ${schema.exclusiveMaximum}` });
  }
}

function matchesType(value: unknown, type: string): boolean {
  switch (type) {
    case "object":
      return isPlainObject(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "number":
      return typeof value === "number";
    case "integer":
      return typeof value === "number" && Number.isInteger(value);
    case "boolean":
      return typeof value === "boolean";
    default:
      return true;
  }
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function describe(value: unknown): string {
  if (Array.isArray(value)) {
    return "an array";
  }
  if (isPlainObject(value)) {
    return "a table";
  }
  return typeof value === "string" ? "a string" : typeof value === "number" ? "a number" : typeof value === "boolean" ? "a boolean" : "a value";
}

function article(type: string): string {
  return type === "object" || type === "array" || type === "integer" ? "an" : "a";
}
