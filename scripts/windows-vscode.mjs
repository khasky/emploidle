import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import {
  automationRequest,
  clickEditorPoint,
  focusWindow,
  openPreparedEditor,
  scrollEditorFile,
} from "./lib/windows-automation.mjs";

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
  [
    "lib/helper.mjs",
    Array.from(
      { length: 400 },
      (_, index) => `export const value${index} = ${index};\n`,
    ).join(""),
  ],
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
$tabs = @(); $view = ''; $folders = @(); $treeSelection = @()
foreach ($element in $elements) {
 if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::TabItem) {
  $selection = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
  if ($element.Current.ClassName -match '^tab(\\s|$)') { $tabs += @{name=$element.Current.Name; id=@($element.GetRuntimeId()); selected=$selection.Current.IsSelected} }
  elseif ($element.Current.Name -match '^(Explorer|Extensions)' -and $selection.Current.IsSelected) { $view = $element.Current.Name }
 }
 if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::TreeItem -and $element.Current.Name -eq 'lib') {
  $folders += @{name=$element.Current.Name; state=$element.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState.ToString()}
 }
 if ($element.Current.ControlType -eq [System.Windows.Automation.ControlType]::Tree -and $element.Current.Name -eq 'Files Explorer') {
  $treeSelection = @($element.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection() | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
 }
}
@{window=$app.MainWindowHandle.ToInt64(); process_id=$app.Id; tabs=@($tabs); view=$view; folders=@($folders);treeSelection=@($treeSelection)} | ConvertTo-Json -Depth 8 -Compress
`),
  );
const viewportImage = ({ x, y, width, height }) =>
  powershell(`
Add-Type -AssemblyName System.Drawing
$image = [Drawing.Bitmap]::new(${width - 120},${height - 120})
$graphics = [Drawing.Graphics]::FromImage($image)
$stream = [IO.MemoryStream]::new()
try {
 $graphics.CopyFromScreen(${x + 90},${y + 60},0,0,$image.Size)
 $image.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
 [Convert]::ToBase64String([Security.Cryptography.SHA256]::Create().ComputeHash($stream.ToArray()))
} finally { $graphics.Dispose();$image.Dispose();$stream.Dispose() }
`).trim();
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
  const prepared = run({ operation: "begin_editor", action: 2 }).session;
  assert.ok(prepared?.file, "A project file was not prepared");
  const opened = openPreparedEditor(handle, prepared);
  assert.ok(opened.ok, opened.error);
  const file = opened.data;
  assert.ok(file?.created_editor, "A project file was not opened");
  assert.equal(state().tabs.length, originalTabs.length + 1);
  assert.equal(
    run({
      operation: "begin_editor",
      action: 2,
      exclude_files: ["notes.md", "helper.mjs"].filter(
        (name) => name !== prepared.file.name,
      ),
    }).session,
    null,
    "An already open visible file was selected again",
  );
  run({ operation: "restore_editor", session: file, interrupted: false });
  restored();
  const rollbackPrepared = run({
    operation: "begin_editor",
    action: 2,
  }).session;
  const rolledBack = openPreparedEditor(handle, rollbackPrepared, false);
  assert.ok(rolledBack.ok, rolledBack.error);
  assert.ok(rolledBack.data.created_editor);
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
  const visitedFiles = [];
  for (let index = 0; index < 2; index++) {
    const preparation = run({
      operation: "begin_editor",
      action: 2,
      exclude_files: visitedFiles,
    });
    const preparedFile = preparation.session;
    assert.ok(
      preparedFile?.file,
      `Collapsed folders prevented project file discovery: ${JSON.stringify({ reason: preparation.reason, visitedFiles, state: state() })}`,
    );
    assert.ok(
      !visitedFiles.includes(preparedFile.file.name),
      "The same file was reviewed twice in one scenario",
    );
    visitedFiles.push(preparedFile.file.name);
    const openedFile = openPreparedEditor(handle, preparedFile);
    assert.ok(openedFile.ok, openedFile.error);
    run({
      operation: "restore_editor",
      session: openedFile.data,
      interrupted: false,
    });
    restored();
    assert.deepEqual(
      state().folders,
      initial.folders,
      "File review left a project folder expanded",
    );
  }
  assert.deepEqual(visitedFiles.slice().sort(), ["helper.mjs", "notes.md"]);
  powershell(`
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]${handle.window})
$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
 [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::TreeItem))
$notes = $items | Where-Object { $_.Current.Name -eq 'notes.md' } | Select-Object -First 1
$notes.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
$notes.SetFocus()
$folder = $items | Where-Object { $_.Current.Name -eq 'lib' } | Select-Object -First 1
$folder.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
$folder.SetFocus()
$folder.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
`);
  const previewBefore = state();
  assert.ok(
    previewBefore.tabs.some((tab) => tab.name.includes(", preview")),
    "The preview fixture did not open",
  );
  const previewPrepared = run({ operation: "begin_editor", action: 2 }).session;
  assert.ok(
    previewPrepared?.file,
    "An existing preview prevented file preparation",
  );
  const pinned = openPreparedEditor(handle, previewPrepared);
  assert.ok(pinned.ok, pinned.error);
  const during = state();
  const viewport = run({ operation: "editor_viewport", session: pinned.data });
  const imageBefore = viewportImage(viewport);
  scrollEditorFile(handle, pinned.data, 8);
  const imageAfter = viewportImage(viewport);
  assert.notEqual(
    imageAfter,
    imageBefore,
    "The owned long project file never scrolled",
  );
  scrollEditorFile(handle, pinned.data, -8);
  assert.equal(
    viewportImage(viewport),
    imageBefore,
    "The project file did not return to its original viewport",
  );
  for (const original of previewBefore.tabs)
    assert.deepEqual(
      during.tabs.find(
        (tab) => JSON.stringify(tab.id) === JSON.stringify(original.id),
      )?.name,
      original.name,
      "A preexisting preview was replaced or pinned",
    );
  run({
    operation: "restore_editor",
    session: pinned.data,
    interrupted: false,
  });
  assert.deepEqual(
    state().tabs,
    previewBefore.tabs,
    "The preexisting preview was not restored exactly",
  );
  for (const [name, text] of contents)
    assert.equal(readFileSync(join(workspace, name), "utf8"), text);

  const blankPoint = JSON.parse(
    powershell(`
Add-Type -AssemblyName UIAutomationClient
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]${handle.window})
$trees = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Tree))
$tree = $trees | Where-Object {$_.Current.Name -eq 'Files Explorer'} | Select-Object -First 1
$bounds=$tree.Current.BoundingRectangle
@{x=[int]($bounds.Left+$bounds.Width/2);y=[int]($bounds.Bottom-8)}|ConvertTo-Json -Compress
`),
  );
  clickEditorPoint(handle, blankPoint);
  assert.equal(
    state().treeSelection.length,
    0,
    "The empty tree selection fixture was not created",
  );
  const emptySelectionPreparation = run({
    operation: "begin_editor",
    action: 2,
  });
  const emptySelectionFile = emptySelectionPreparation.session;
  assert.ok(
    emptySelectionFile?.file,
    `An empty Explorer selection prevented file review: ${emptySelectionPreparation.reason}`,
  );
  const emptySelectionOpened = openPreparedEditor(handle, emptySelectionFile);
  assert.ok(emptySelectionOpened.ok, emptySelectionOpened.error);
  run({
    operation: "restore_editor",
    session: emptySelectionOpened.data,
    interrupted: false,
  });
  assert.equal(
    state().treeSelection.length,
    0,
    "The empty Explorer selection was changed",
  );

  powershell(`
Add-Type -AssemblyName System.Windows.Forms
[Windows.Forms.SendKeys]::SendWait('^\\')
`);
  const grouped = state();
  assert.ok(
    grouped.tabs.filter((tab) => tab.selected).length >= 2,
    "The split-editor fixture was not created",
  );
  const groupPreparation = run({ operation: "begin_editor", action: 2 });
  const groupFile = groupPreparation.session;
  assert.ok(
    groupFile?.file,
    `Multiple editor groups prevented file review: ${groupPreparation.reason}`,
  );
  const groupOpened = openPreparedEditor(handle, groupFile);
  assert.ok(groupOpened.ok, groupOpened.error);
  run({ operation: "editor_viewport", session: groupOpened.data });
  run({
    operation: "restore_editor",
    session: groupOpened.data,
    interrupted: false,
  });
  assert.deepEqual(
    state().tabs,
    grouped.tabs,
    "File review changed the original editor group selections",
  );

  powershell(`
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Windows.Forms
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]${handle.window})
[Windows.Forms.SendKeys]::SendWait('^k^w')
$timer = [Diagnostics.Stopwatch]::StartNew()
do {
 $tabs = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::TabItem))
 $editors = @($tabs | Where-Object {$_.Current.ClassName -match '^tab(\\s|$)'})
 if($editors.Count -eq 0) {break}
 Start-Sleep -Milliseconds 15
} while($timer.ElapsedMilliseconds -lt 700)
if($editors.Count -gt 0) {throw 'The fixture editors did not close'}
`);
  assert.equal(
    state().tabs.length,
    0,
    "The empty editor fixture was not created",
  );
  const emptyEditorPreparation = run({ operation: "begin_editor", action: 2 });
  const emptyEditorFile = emptyEditorPreparation.session;
  assert.ok(
    emptyEditorFile?.file,
    `An editor without open tabs prevented file review: ${emptyEditorPreparation.reason}`,
  );
  const emptyEditorOpened = openPreparedEditor(handle, emptyEditorFile);
  assert.ok(emptyEditorOpened.ok, emptyEditorOpened.error);
  run({
    operation: "restore_editor",
    session: emptyEditorOpened.data,
    interrupted: false,
  });
  assert.equal(
    state().tabs.length,
    0,
    "The created tab remained in the originally empty editor",
  );
  for (const [name, text] of contents)
    assert.equal(readFileSync(join(workspace, name), "utf8"), text);
  console.log(
    "PASS: native VS Code file rotation, scrolling, sidebar/folder restoration, preview protection, empty selection, split groups, empty editors and acknowledgement rollback",
  );
} finally {
  child.kill();
  await exited;
}
