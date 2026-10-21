// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Entry point. Each feature registers its own providers and commands; the data they share is
// loaded once from the bundled schema and registry.

import * as vscode from "vscode";
import { loadBundledData } from "./core/data";
import { registerAuditViewer } from "./auditViewer";
import { registerContractExplorer } from "./contractExplorer";
import { registerErrorHover } from "./errorHover";
import { registerManifestFeatures } from "./manifestFeatures";
import { registerRunConfigs } from "./runConfigs";

export function activate(context: vscode.ExtensionContext): void {
  const data = loadBundledData(context.extensionPath);
  registerManifestFeatures(context, data);
  registerErrorHover(context, data);
  registerContractExplorer(context);
  registerRunConfigs(context);
  registerAuditViewer(context);
}

export function deactivate(): void {
  // Nothing to release: every disposable is in context.subscriptions.
}
