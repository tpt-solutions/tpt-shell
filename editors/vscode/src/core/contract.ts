// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Reads the contract.json that the app writes (tpt-shell-ipc, `Contract::to_json_string`). The
// explorer only browses it. Calls are made by the devtools overlay, so this module builds the
// copy-able argument JSON and never sends anything.

export interface ContractField {
  name: string;
  type: string;
  required: boolean;
  constraints: string[];
}

export interface ContractCommand {
  name: string;
  kind: string;
  capability: string | null;
  ui: boolean;
  agent: boolean;
  args: JsonSchema;
}

export interface Contract {
  version: number;
  commands: ContractCommand[];
}

export type JsonSchema = {
  type?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  items?: JsonSchema;
  enum?: unknown[];
  anyOf?: JsonSchema[];
  additionalProperties?: boolean | JsonSchema;
  minimum?: number;
  maximum?: number;
  maxLength?: number;
  minLength?: number;
  maxItems?: number;
  maxProperties?: number;
  pattern?: string;
  [key: string]: unknown;
};

export type ContractParse = { ok: true; contract: Contract } | { ok: false; message: string };

export function parseContract(text: string): ContractParse {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (error) {
    return { ok: false, message: `not valid JSON: ${(error as Error).message}` };
  }
  if (!isObject(raw) || !Array.isArray(raw.commands)) {
    return { ok: false, message: "no \"commands\" array at the top level" };
  }
  if (typeof raw.contract_version !== "number") {
    return { ok: false, message: "no \"contract_version\" number at the top level" };
  }
  const commands: ContractCommand[] = [];
  for (const item of raw.commands) {
    if (!isObject(item) || typeof item.name !== "string") {
      return { ok: false, message: "a command has no \"name\"" };
    }
    const flags = isObject(item.flags) ? item.flags : {};
    commands.push({
      name: item.name,
      kind: String(item.kind),
      capability: typeof item.capability === "string" ? item.capability : null,
      ui: flags.ui === true,
      agent: flags.agent === true,
      args: isObject(item.args) ? (item.args as JsonSchema) : {},
    });
  }
  return { ok: true, contract: { version: raw.contract_version, commands } };
}

// The fields of a command's argument object, with the constraints that are written in its schema.
export function fieldsOf(schema: JsonSchema): ContractField[] {
  const required = new Set(schema.required ?? []);
  return Object.entries(schema.properties ?? {}).map(([name, node]) => {
    const inner = unwrapOptional(node);
    return {
      name,
      type: describeType(inner),
      required: required.has(name),
      constraints: constraintsOf(inner),
    };
  });
}

// A short type name for a schema node, for example `string`, `int`, `list<string>`.
export function describeType(schema: JsonSchema): string {
  if (schema.enum) {
    return `one of ${schema.enum.map((option) => JSON.stringify(option)).join(" | ")}`;
  }
  switch (schema.type) {
    case "integer":
      return "int";
    case "array":
      return schema.items ? `list<${describeType(schema.items)}>` : "list";
    case "object":
      if (schema.additionalProperties && typeof schema.additionalProperties === "object") {
        return `map<string, ${describeType(schema.additionalProperties)}>`;
      }
      return schema.properties ? "struct" : "object";
    default:
      return schema.type ?? "any";
  }
}

function constraintsOf(schema: JsonSchema): string[] {
  const out: string[] = [];
  if (schema.minimum !== undefined || schema.maximum !== undefined) {
    out.push(`range ${schema.minimum ?? "-inf"} to ${schema.maximum ?? "inf"}`);
  }
  if (schema.minLength !== undefined || schema.maxLength !== undefined) {
    out.push(`length ${schema.minLength ?? 0} to ${schema.maxLength ?? "any"}`);
  }
  if (schema.pattern !== undefined) {
    out.push(`pattern ${schema.pattern}`);
  }
  if (schema.maxItems !== undefined) {
    out.push(`at most ${schema.maxItems} items`);
  }
  return out;
}

// A field that may be absent is written as anyOf [type, null] by the contract.
export function unwrapOptional(schema: JsonSchema): JsonSchema {
  if (Array.isArray(schema.anyOf)) {
    const inner = schema.anyOf.find((option) => option.type !== "null");
    if (inner) {
      return inner;
    }
  }
  return schema;
}

// An argument object that satisfies the schema's required fields. Optional fields are left out.
export function exampleArgs(schema: JsonSchema): unknown {
  const inner = unwrapOptional(schema);
  if (inner.enum) {
    return inner.enum[0];
  }
  switch (inner.type) {
    case "object": {
      const out: Record<string, unknown> = {};
      const required = new Set(inner.required ?? []);
      for (const [name, node] of Object.entries(inner.properties ?? {})) {
        if (required.has(name)) {
          out[name] = exampleArgs(node);
        }
      }
      return out;
    }
    case "array":
      return [];
    case "string":
      return "";
    case "integer":
    case "number":
      return inner.minimum ?? 0;
    case "boolean":
      return false;
    default:
      return null;
  }
}

// The copy-able call text for a command: its name and an argument template.
export function callJson(command: ContractCommand): string {
  return JSON.stringify({ command: command.name, args: exampleArgs(command.args) }, null, 2);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

