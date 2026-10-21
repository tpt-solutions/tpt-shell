// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Reads the JSON Lines audit file that tpt-shell-audit writes: one record per line with seq,
// wall_unix_ms, actor, command, capability, outcome, error_code, duration_us and prev.

export interface AuditRecord {
  line: number; // 1-based line in the file
  seq: number | null;
  time: string | null; // ISO 8601, from wall_unix_ms
  actor: string;
  command: string;
  capability: string | null;
  outcome: string;
  errorCode: string | null;
  durationUs: number | null;
}

export interface AuditProblem {
  line: number;
  message: string;
}

export interface AuditParse {
  records: AuditRecord[];
  problems: AuditProblem[];
}

export function parseAuditLines(text: string): AuditParse {
  const records: AuditRecord[] = [];
  const problems: AuditProblem[] = [];
  text.split(/\r?\n/).forEach((raw, index) => {
    const line = index + 1;
    if (raw.trim() === "") {
      return;
    }
    let value: unknown;
    try {
      value = JSON.parse(raw);
    } catch (error) {
      problems.push({ line, message: `not valid JSON: ${(error as Error).message}` });
      return;
    }
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      problems.push({ line, message: "the line is not a JSON object" });
      return;
    }
    const record = value as Record<string, unknown>;
    if (typeof record.command !== "string" || typeof record.outcome !== "string") {
      problems.push({ line, message: "the record has no command or outcome" });
      return;
    }
    records.push({
      line,
      seq: typeof record.seq === "number" ? record.seq : null,
      time: isoTime(record.wall_unix_ms),
      actor: typeof record.actor === "string" ? record.actor : "",
      command: record.command,
      capability: typeof record.capability === "string" ? record.capability : null,
      outcome: record.outcome,
      errorCode: typeof record.error_code === "string" ? record.error_code : null,
      durationUs: typeof record.duration_us === "number" ? record.duration_us : null,
    });
  });
  return { records, problems };
}

function isoTime(value: unknown): string | null {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}
