// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

// Run configurations for `tpt-shell dev` and `tpt-shell build`. Both run as tasks in a terminal,
// so the app's output is the terminal's output; there is no debugger. The app has no debug
// protocol yet, so a launch configuration of type "tpt-shell" starts the same task and then
// ends the debug session. See the report for the reason.

import * as vscode from "vscode";
import { runArgs, RunDefinition } from "./core/runArgs";

const TASK_TYPE = "tpt-shell";

function cliPath(): string {
  return vscode.workspace.getConfiguration("tpt-shell").get<string>("cliPath", "tpt-shell") || "tpt-shell";
}

function defaultDirectory(): string {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? ".";
}

// The task for a definition, or a message when the definition is not usable.
function taskFor(definition: RunDefinition, scope: vscode.WorkspaceFolder | vscode.TaskScope): vscode.Task | string {
  const built = runArgs(definition, defaultDirectory());
  if (!built.ok) {
    return built.message;
  }
  const task = new vscode.Task(
    { type: TASK_TYPE, command: definition.command, directory: definition.directory, port: definition.port },
    scope,
    `tpt-shell ${definition.command}`,
    TASK_TYPE,
    new vscode.ShellExecution(cliPath(), built.args)
  );
  task.isBackground = built.background;
  task.presentationOptions = { reveal: vscode.TaskRevealKind.Always, panel: vscode.TaskPanelKind.Dedicated };
  return task;
}

async function runDefinition(definition: RunDefinition): Promise<void> {
  const scope = vscode.workspace.workspaceFolders?.[0] ?? vscode.TaskScope.Workspace;
  const task = taskFor(definition, scope);
  if (typeof task === "string") {
    void vscode.window.showErrorMessage(`tpt-shell: ${task}`);
    return;
  }
  await vscode.tasks.executeTask(task);
}

export function registerRunConfigs(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.tasks.registerTaskProvider(TASK_TYPE, {
      provideTasks() {
        const scope = vscode.workspace.workspaceFolders?.[0] ?? vscode.TaskScope.Workspace;
        return ["dev", "build"]
          .map((command) => taskFor({ command }, scope))
          .filter((task): task is vscode.Task => typeof task !== "string");
      },
      resolveTask(task) {
        const definition = task.definition as unknown as RunDefinition;
        const scope = vscode.workspace.workspaceFolders?.[0] ?? vscode.TaskScope.Workspace;
        const resolved = taskFor(definition, scope);
        return typeof resolved === "string" ? undefined : resolved;
      },
    }),
    vscode.debug.registerDebugConfigurationProvider(TASK_TYPE, {
      async resolveDebugConfiguration(_folder, config) {
        if (!config.command) {
          void vscode.window.showErrorMessage("tpt-shell launch configuration needs \"command\": \"dev\" or \"build\".");
          return undefined;
        }
        await runDefinition({ command: config.command, directory: config.directory, port: config.port });
        // No debug adapter exists for the app, so the session is not started.
        return undefined;
      },
    }),
    vscode.commands.registerCommand("tpt-shell.runDev", () => runDefinition({ command: "dev" })),
    vscode.commands.registerCommand("tpt-shell.runBuild", () => runDefinition({ command: "build" }))
  );
}
