// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// The contract explorer: a tree of the commands in a contract.json the app wrote. It browses and
// copies call JSON. Calls are made by the devtools overlay, not from here.

import * as fs from "fs";
import * as vscode from "vscode";
import { callJson, ContractCommand, ContractField, fieldsOf, parseContract } from "./core/contract";

type Node =
  | { kind: "message"; label: string }
  | { kind: "command"; command: ContractCommand }
  | { kind: "detail"; label: string; description: string }
  | { kind: "fields"; command: ContractCommand }
  | { kind: "field"; field: ContractField };

class ContractTree implements vscode.TreeDataProvider<Node> {
  private readonly changed = new vscode.EventEmitter<Node | undefined>();
  readonly onDidChangeTreeData = this.changed.event;
  private roots: Node[] = [];
  private file: string | undefined;

  async reload(): Promise<void> {
    this.file = await findContractFile();
    if (!this.file) {
      this.roots = [{ kind: "message", label: "No contract.json found. Set tpt-shell.contractFile or open one." }];
      this.changed.fire(undefined);
      return;
    }
    let text: string;
    try {
      text = fs.readFileSync(this.file, "utf8");
    } catch (error) {
      this.roots = [{ kind: "message", label: `Cannot read ${this.file}: ${(error as Error).message}` }];
      this.changed.fire(undefined);
      return;
    }
    const parsed = parseContract(text);
    if (!parsed.ok) {
      this.roots = [{ kind: "message", label: `Not a contract file: ${parsed.message}` }];
    } else {
      this.roots = parsed.contract.commands.map((command): Node => ({ kind: "command", command }));
      if (this.roots.length === 0) {
        this.roots = [{ kind: "message", label: "The contract has no commands." }];
      }
    }
    this.changed.fire(undefined);
  }

  get source(): string | undefined {
    return this.file;
  }

  getTreeItem(node: Node): vscode.TreeItem {
    switch (node.kind) {
      case "message":
        return new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.None);
      case "command": {
        const item = new vscode.TreeItem(node.command.name, vscode.TreeItemCollapsibleState.Collapsed);
        item.description = `${node.command.kind}${node.command.capability ? ` · ${node.command.capability}` : ""}`;
        item.contextValue = "command";
        item.tooltip = `${node.command.name}\nkind: ${node.command.kind}\ncapability: ${node.command.capability ?? "none"}`;
        item.iconPath = new vscode.ThemeIcon("symbol-method");
        return item;
      }
      case "detail": {
        const item = new vscode.TreeItem(`${node.label}: ${node.description}`, vscode.TreeItemCollapsibleState.None);
        return item;
      }
      case "fields": {
        const count = fieldsOf(node.command.args).length;
        return new vscode.TreeItem(`arguments (${count})`, count > 0 ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.None);
      }
      case "field": {
        const item = new vscode.TreeItem(node.field.name, vscode.TreeItemCollapsibleState.None);
        item.description = [node.field.type, node.field.required ? "required" : "optional", ...node.field.constraints].join(", ");
        return item;
      }
    }
  }

  getChildren(node?: Node): Node[] {
    if (!node) {
      return this.roots;
    }
    if (node.kind === "command") {
      const flags = [node.command.agent ? "agent" : "", node.command.ui ? "ui" : ""].filter(Boolean).join(", ");
      return [
        { kind: "detail", label: "kind", description: node.command.kind },
        { kind: "detail", label: "capability", description: node.command.capability ?? "none" },
        { kind: "detail", label: "flags", description: flags || "none" },
        { kind: "fields", command: node.command },
      ];
    }
    if (node.kind === "fields") {
      return fieldsOf(node.command.args).map((field): Node => ({ kind: "field", field }));
    }
    return [];
  }
}

async function findContractFile(): Promise<string | undefined> {
  const configured = vscode.workspace.getConfiguration("tpt-shell").get<string>("contractFile", "").trim();
  if (configured) {
    return configured;
  }
  const found = await vscode.workspace.findFiles("**/contract.json", "**/node_modules/**", 20);
  return found[0]?.fsPath;
}

export function registerContractExplorer(context: vscode.ExtensionContext): void {
  const tree = new ContractTree();
  const view = vscode.window.createTreeView("tptShell.contract", { treeDataProvider: tree, showCollapseAll: true });
  context.subscriptions.push(view);
  void tree.reload();

  const watcher = vscode.workspace.createFileSystemWatcher("**/contract.json");
  watcher.onDidChange(() => void tree.reload());
  watcher.onDidCreate(() => void tree.reload());
  watcher.onDidDelete(() => void tree.reload());
  context.subscriptions.push(
    watcher,
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("tpt-shell.contractFile")) {
        void tree.reload();
      }
    }),
    vscode.commands.registerCommand("tpt-shell.refreshContract", () => tree.reload()),
    vscode.commands.registerCommand("tpt-shell.openContract", async () => {
      const picked = await vscode.window.showOpenDialog({
        canSelectMany: false,
        filters: { "Contract JSON": ["json"] },
        openLabel: "Use this contract",
      });
      if (!picked || picked.length === 0) {
        return;
      }
      const target = vscode.workspace.workspaceFolders ? vscode.ConfigurationTarget.Workspace : vscode.ConfigurationTarget.Global;
      await vscode.workspace.getConfiguration("tpt-shell").update("contractFile", picked[0].fsPath, target);
      await tree.reload();
    }),
    vscode.commands.registerCommand("tpt-shell.copyCallJson", async (node?: { kind?: string; command?: ContractCommand }) => {
      const command = node?.command;
      if (!command) {
        return;
      }
      await vscode.env.clipboard.writeText(callJson(command));
      void vscode.window.setStatusBarMessage(`Copied the call JSON for ${command.name}`, 3000);
    })
  );
}
