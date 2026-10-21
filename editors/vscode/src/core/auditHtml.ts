// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The audit viewer page. It is plain HTML with no script and no external resources, so the
// webview's content security policy can stay closed.

import { AuditParse } from "./audit";
import { VerifyResult, VerifyStatus } from "./verify";

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const BANNER: Record<VerifyStatus, string> = {
  verified: "Verified: the hash chain checks out",
  broken: "Not verified: the hash chain or a signature does not check out",
  error: "Verification could not run",
  unavailable: "Not verified: the tpt-shell CLI was not found, so the chain was not checked",
};

export function renderAuditPage(fileName: string, parsed: AuditParse, verify: VerifyResult): string {
  const rows = parsed.records
    .map(
      (record) => `<tr>
  <td>${record.seq ?? ""}</td>
  <td>${escapeHtml(record.time ?? "")}</td>
  <td>${escapeHtml(record.actor)}</td>
  <td>${escapeHtml(record.command)}</td>
  <td>${escapeHtml(record.capability ?? "")}</td>
  <td class="${record.outcome === "ok" ? "" : "bad"}">${escapeHtml(record.outcome)}${
        record.errorCode ? ` (${escapeHtml(record.errorCode)})` : ""
      }</td>
  <td class="num">${record.durationUs ?? ""}</td>
  <td class="num">${record.line}</td>
</tr>`
    )
    .join("\n");
  const problems = parsed.problems
    .map((problem) => `<li>line ${problem.line}: ${escapeHtml(problem.message)}</li>`)
    .join("");
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(fileName)}</title>
<style>
  body { font-family: var(--vscode-font-family); color: var(--vscode-foreground); background: var(--vscode-editor-background); padding: 16px; }
  h1 { font-size: 1.2em; font-weight: 600; margin: 0 0 8px; }
  .status { padding: 8px 12px; border-left: 4px solid; margin-bottom: 12px; }
  .status.verified { border-color: var(--vscode-testing-iconPassed, #3fb950); }
  .status.broken, .status.error, .status.unavailable { border-color: var(--vscode-testing-iconFailed, #f85149); }
  .detail { white-space: pre-wrap; font-family: var(--vscode-editor-font-family); font-size: 0.9em; margin-top: 4px; }
  table { border-collapse: collapse; width: 100%; font-size: 0.92em; }
  th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--vscode-panel-border, #8884); vertical-align: top; }
  th { font-weight: 600; }
  td.num { text-align: right; font-variant-numeric: tabular-nums; }
  td.bad { color: var(--vscode-errorForeground, #f85149); }
  .problems { color: var(--vscode-errorForeground, #f85149); }
  .count { opacity: 0.8; margin-bottom: 8px; }
</style>
</head>
<body>
<h1>${escapeHtml(fileName)}</h1>
<div class="status ${verify.status}">
  ${escapeHtml(BANNER[verify.status])}
  <div class="detail">${escapeHtml(verify.message)}</div>
</div>
<div class="count">${parsed.records.length} record(s)${parsed.problems.length ? `, ${parsed.problems.length} unreadable line(s)` : ""}</div>
${problems ? `<ul class="problems">${problems}</ul>` : ""}
<table>
<thead>
<tr><th>seq</th><th>time (UTC)</th><th>actor</th><th>command</th><th>capability</th><th>outcome</th><th>duration (us)</th><th>line</th></tr>
</thead>
<tbody>
${rows}
</tbody>
</table>
</body>
</html>
`;
}
