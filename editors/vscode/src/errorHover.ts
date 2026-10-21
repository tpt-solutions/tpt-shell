// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Hover on a TS#### code in the manifest, Rust sources and logs. It shows the registry entry, the
// same text `tpt-shell explain` prints.

import * as vscode from "vscode";
import { BundledData } from "./core/data";
import { codeAt, explainMarkdown } from "./core/registry";

export function registerErrorHover(context: vscode.ExtensionContext, data: BundledData): void {
  const selector: vscode.DocumentSelector = [
    { language: "toml" },
    { language: "rust" },
    { language: "json" },
    { language: "log" },
    { language: "plaintext" },
    { pattern: "**/*.log" },
  ];
  context.subscriptions.push(
    vscode.languages.registerHoverProvider(selector, {
      provideHover(document, position) {
        const text = document.lineAt(position.line).text;
        const match = codeAt(text, position.character);
        if (!match) {
          return undefined;
        }
        const entry = data.registry.lookup(match.code);
        if (!entry) {
          return new vscode.Hover(`${match.code} is not in the error registry.`);
        }
        const range = new vscode.Range(position.line, match.start, position.line, match.end);
        return new vscode.Hover(new vscode.MarkdownString(explainMarkdown(entry)), range);
      },
    })
  );
}
