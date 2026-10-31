import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { automationRequest, focusWindow } from "./lib/windows-automation.mjs";

assert.equal(process.platform, "win32");
const executable = [
  join(process.env.ProgramFiles, "Microsoft VS Code/Code.exe"),
  join(process.env.LOCALAPPDATA, "Programs/Microsoft VS Code/Code.exe"),
].find(existsSync);
assert.ok(
  executable,
  "VS Code must be installed for the native editor integration check",
);
const directory = resolve(
  `src-tauri/target/vscode-verification-${process.pid}`,
);
const workspace = join(directory, "workspace");
for (const path of ["workspace/lib", "profile/User", "extensions", "shared"])
  mkdirSync(join(directory, path), { recursive: true });
const contents = new Map([
  ["main.js", 'console.log("Emploidle verification");\n'],
  ["notes.md", "# Verification notes\n\nTemporary integration fixture.\n"],
  ["lib/helper.js", "export const value = 42;\n"],
]);
for (const [name, text] of contents) writeFileSync(join(workspace, name), text);
writeFileSync(
  join(directory, "profile/User/settings.json"),
  JSON.stringify({
    "workbench.startupEditor": "none",
    "update.mode": "none",
    "extensions.autoUpdate": false,
    "extensions.autoCheckUpdates": false,
    "telemetry.telemetryLevel": "off",
    "security.workspace.trust.enabled": false,
    "editor.accessibilitySupport": "on",
    "files.autoSave": "off",
    "window.title": `Emploidle editor verification ${process.pid}`,
  }),
);
const child = spawn(
  executable,
  [
    "--new-window",
    "--user-data-dir",
    join(directory, "profile"),
    "--shared-data-dir",
    join(directory, "shared"),
    "--extensions-dir",
    join(directory, "extensions"),
    "--disable-extensions",
    "--skip-welcome",
    "--skip-release-notes",
    "--disable-workspace-trust",
    "--force-renderer-accessibility",
    workspace,
    join(workspace, "main.js"),
  ],
  { windowsHide: false, stdio: "ignore" },
);
const exited = new Promise((accept, reject) => {
  child.once("exit", accept);
  child.once("error", reject);
});
const powershell = (script) =>
  execFileSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    { encoding: "utf8", windowsHide: true },
  );
const state = () =>
  JSON.parse(
    powershell(`
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$app = Get-Process -Id ${child.pid}
if ($app.MainWindowHandle -eq 0) { 'null'; exit }
$root = [System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
$elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$tabs = @(); $view = ''; $folders = @()
foreach ($element in $elements) {
 if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::TabItem) {
  $selection = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
  if ($element.Current.ClassName -match '^tab(\\s|$)') { $tabs += @{name=$element.Current.Name; id=@($element.GetRuntimeId()); selected=$selection.Current.IsSelected} }
  elseif ($element.Current.Name -match '^(Explorer|Extensions)' -and $selection.Current.IsSelected) { $view = $element.Current.Name }
 }
 if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::TreeItem -and $element.Current.Name -eq 'lib') {
  $folders += @{name=$element.Current.Name; state=$element.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState.ToString()}
 }
}
@{window=$app.MainWindowHandle.ToInt64(); process_id=$app.Id; tabs=@($tabs); view=$view; folders=@($folders)} | ConvertTo-Json -Depth 8 -Compress
`),
  );
try {
  let initial;
  for (let attempt = 0; attempt < 60; attempt++) {
    initial = state();
    if (
      initial?.view.startsWith("Explorer") &&
      initial.tabs.some((tab) => tab.name === "main.js") &&
      initial.folders.some(
        (folder) => folder.name === "lib" && folder.state === "Collapsed",
      )
    )
      break;
    await delay(150);
  }
  assert.ok(
    initial?.tabs.some((tab) => tab.name === "main.js") &&
      initial.folders.some(
        (folder) => folder.name === "lib" && folder.state === "Collapsed",
      ),
    "The owned editor did not finish loading",
  );
  const handle = { window: initial.window, process_id: initial.process_id };
  focusWindow(handle);
  const request = (operation, acknowledge = true) =>
    automationRequest(handle, operation, acknowledge);
  const run = (operation, acknowledge = true) => {
    const response = request(operation, acknowledge);
    assert.ok(response.ok, response.error);
    return response.data;
  };
  const originalTabs = initial.tabs.map((tab) => tab.id);
  const restored = () => {
    const current = state();
    assert.deepEqual(
      current.tabs.map((tab) => tab.id),
      originalTabs,
      "Original editor tabs were changed",
    );
    assert.ok(
      current.view.startsWith("Explorer"),
      "Original sidebar was not restored",
    );
    assert.equal(current.tabs.find((tab) => tab.selected)?.name, "main.js");
    for (const [name, text] of contents)
      assert.equal(
        readFileSync(join(workspace, name), "utf8"),
        text,
        `Project file changed: ${name}`,
      );
  };
  const extensions = run({ operation: "begin_editor", action: 0 }).session;
  assert.ok(extensions, "Extensions review was skipped");
  assert.ok(state().view.startsWith("Extensions"));
  run({ operation: "restore_editor", session: extensions, interrupted: true });
  assert.ok(
    state().view.startsWith("Extensions"),
    "Interrupted cleanup changed the sidebar",
  );
  run({
    operation: "restore_editor",
    session: extensions,
    interrupted: false,
    input_tick: 0,
  });
  assert.ok(
    state().view.startsWith("Extensions"),
    "Stale cleanup changed the sidebar",
  );
  run({ operation: "restore_editor", session: extensions, interrupted: false });
  restored();
  assert.ok(run({ operation: "begin_editor", action: 0 }, false).session);
  restored();
  const file = run({ operation: "begin_editor", action: 2 }).session;
  assert.ok(file?.created_editor, "A project file was not opened");
  assert.equal(state().tabs.length, originalTabs.length + 1);
  assert.equal(
    run({ operation: "begin_editor", action: 2 }).session,
    null,
    "A preexisting preview was replaced",
  );
  run({ operation: "restore_editor", session: file, interrupted: false });
  restored();
  assert.ok(
    run({ operation: "begin_editor", action: 2 }, false).session
      ?.created_editor,
  );
  restored();
  assert.equal(
    run({ operation: "review_tree", index: 0 }),
    true,
    "Project folder review did not run",
  );
  assert.deepEqual(
    state().folders,
    initial.folders,
    "Folder expansion state was changed",
  );
  restored();
  console.log(
    "PASS: native VS Code sidebar restoration, owned project-file cleanup, preview protection, acknowledgement rollback and folder restoration",
  );
} finally {
  child.kill();
  await exited;
}
