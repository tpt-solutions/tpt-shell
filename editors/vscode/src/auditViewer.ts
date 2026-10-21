// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The audit viewer: opens a JSON Lines audit file in a webview with its entries and the result of
// `tpt-shell audit verify`. The verify status is the CLI's, not a second implementation.

import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { parseAuditLines } from "./core/audit";
import { renderAuditPage } from "./core/auditHtml";
import { runVerify } from "./core/verify";

async function pickAuditFile(): Promise<string | undefined> {
  const active = vscode.window.activeTextEditor?.document;
  if (active && active.uri.scheme === "file" && active.fileName.endsWith(".jsonl")) {
    return active.fileName;
  }
  const picked = await vscode.window.showOpenDialog({
    canSelectMany: false,
    filters: { "Audit log": ["jsonl"] },
    openLabel: "Open audit log",
  });
  return picked?.[0]?.fsPath;
}

export function registerAuditViewer(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("tpt-shell.openAuditLog", async (resource?: vscode.Uri) => {
      const file = resource?.fsPath ?? (await pickAuditFile());
      if (!file) {
        return;
      }
      await openAuditLog(file);
    })
  );
}

async function openAuditLog(file: string): Promise<void> {
  let text: string;
  try {
    text = fs.readFileSync(file, "utf8");
  } catch (error) {
    void vscode.window.showErrorMessage(`Cannot read ${file}: ${(error as Error).message}`);
    return;
  }
  const config = vscode.workspace.getConfiguration("tpt-shell");
  const command = config.get<string>("cliPath", "tpt-shell") || "tpt-shell";
  const key = config.get<string>("auditPublicKey", "").trim();

  const panel = vscode.window.createWebviewPanel("tptShell.audit", path.basename(file), vscode.ViewColumn.Active, {
    enableScripts: false,
  });
  panel.webview.html = "<!doctype html><html><body>Verifying the audit log...</body></html>";

  const verify = await runVerify(command, [], file, key || undefined);
  panel.webview.html = renderAuditPage(path.basename(file), parseAuditLines(text), verify);
}
