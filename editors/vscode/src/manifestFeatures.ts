// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Diagnostics, completion and hover for tpt-shell.toml. The checks come from core/manifest.ts,
// which reads the published schema.

import * as vscode from "vscode";
import { BundledData } from "./core/data";
import { checkManifest, completionsAt, hoverAt } from "./core/manifest";

const MANIFEST_NAME = "tpt-shell.toml";

function isManifest(document: vscode.TextDocument): boolean {
  return document.uri.scheme === "file" && document.fileName.split(/[\\/]/).pop() === MANIFEST_NAME;
}

export function registerManifestFeatures(context: vscode.ExtensionContext, data: BundledData): void {
  const diagnostics = vscode.languages.createDiagnosticCollection("tpt-shell");
  context.subscriptions.push(diagnostics);

  const refresh = (document: vscode.TextDocument): void => {
    if (!isManifest(document)) {
      return;
    }
    const found = checkManifest(document.getText(), data.schema).map((problem) => {
      const line = document.lineAt(Math.min(Math.max(problem.line, 1), document.lineCount) - 1);
      const diagnostic = new vscode.Diagnostic(line.range, `${problem.code}: ${problem.message}`, vscode.DiagnosticSeverity.Error);
      diagnostic.code = problem.code;
      diagnostic.source = "tpt-shell";
      return diagnostic;
    });
    diagnostics.set(document.uri, found);
  };

  for (const document of vscode.workspace.textDocuments) {
    refresh(document);
  }
  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument(refresh),
    vscode.workspace.onDidChangeTextDocument((event) => refresh(event.document)),
    vscode.workspace.onDidCloseTextDocument((document) => diagnostics.delete(document.uri))
  );

  const selector: vscode.DocumentSelector = [{ language: "toml", scheme: "file" }];

  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(selector, {
      provideCompletionItems(document, position) {
        if (!isManifest(document)) {
          return undefined;
        }
        const lines = document.getText().split(/\r?\n/);
        return completionsAt(lines, position.line, position.character, data.schema).map((item) => {
          const completion = new vscode.CompletionItem(item.label, kindOf(item.kind));
          completion.detail = item.detail;
          completion.documentation = item.documentation;
          completion.insertText = item.insertText;
          return completion;
        });
      },
    }, "[", "."),
    vscode.languages.registerHoverProvider(selector, {
      provideHover(document, position) {
        if (!isManifest(document)) {
          return undefined;
        }
        const lines = document.getText().split(/\r?\n/);
        const hover = hoverAt(lines, position.line, position.character, data.schema);
        if (!hover) {
          return undefined;
        }
        const markdown = new vscode.MarkdownString(hover.markdown);
        return new vscode.Hover(markdown);
      },
    })
  );
}

function kindOf(kind: "table" | "key" | "value"): vscode.CompletionItemKind {
  switch (kind) {
    case "table":
      return vscode.CompletionItemKind.Module;
    case "key":
      return vscode.CompletionItemKind.Property;
    default:
      return vscode.CompletionItemKind.Value;
  }
}
