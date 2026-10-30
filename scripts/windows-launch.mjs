import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

assert.equal(
  process.platform,
  "win32",
  "This integration check requires Windows",
);
const executable = resolve("src-tauri/target/debug/emploidle.exe");
const image = readFileSync(executable);
assert.equal(
  image.readUInt16LE(image.readUInt32LE(0x3c) + 24 + 68),
  2,
  "Application uses the console subsystem",
);
const names = new Set();

for (let run = 0; run < 2; run++) {
  const launcher = spawn(executable, [], {
    windowsHide: true,
    stdio: "ignore",
  });
  const exited = new Promise((accept, reject) => {
    launcher.once("error", reject);
    launcher.once("exit", accept);
  });
  let application;
  try {
    for (let attempt = 0; attempt < 50; attempt++) {
      // PowerShell supplies the native Windows process and version APIs.
      const query = `
        $ErrorActionPreference = 'Stop'
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = ${launcher.pid}" | Where-Object { $_.Name -match '^p[0-9a-f]{14}\\.exe$' })
        if ($children.Count -gt 1) { throw 'Launcher started multiple applications' }
        if ($children.Count -eq 0) { 'null'; exit }
        $app = Get-Process -Id $children[0].ProcessId
        $metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo($app.Path)
        $browsers = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($app.Id)" | Where-Object { $_.Name -eq 'msedgewebview2.exe' })
        [pscustomobject]@{ id=$app.Id; name=$app.ProcessName; title=$app.MainWindowTitle; window=$app.MainWindowHandle.ToInt64(); description=$metadata.FileDescription; product=$metadata.ProductName; browsers=$browsers.Count } | ConvertTo-Json -Compress
      `;
      application = JSON.parse(
        execFileSync(
          "powershell.exe",
          ["-NoProfile", "-NonInteractive", "-Command", query],
          { encoding: "utf8", windowsHide: true },
        ),
      );
      if (application?.window) {
        assert.equal(
          application.title,
          application.name,
          "Window exposes a fixed title",
        );
        if (application.browsers === 1) break;
      }
      await delay(200);
    }
    assert.ok(application?.window, "Application window did not open");
    assert.equal(await exited, 0, "Launcher failed");
    assert.equal(application.browsers, 1, "Expected one WebView2 browser");
    assert.ok(
      !application.description && !application.product,
      "Task Manager receives fixed metadata",
    );
    assert.ok(!names.has(application.name), "Process name repeated");
    names.add(application.name);
    console.log(
      `PASS: ${application.name}.exe; random window title; GUI subsystem; no fixed description; one WebView2 browser`,
    );
  } finally {
    if (application?.id) process.kill(application.id);
  }
}
