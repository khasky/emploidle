import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";
import mockTauri from "./screenshots/tauri-mock.mjs";

assert.equal(process.platform, "win32", "Desktop integration requires Windows");
const browser = await chromium.launch({
  headless: false,
  args: ["--force-renderer-accessibility"],
});
try {
  const context = await browser.newContext({
    viewport: { width: 440, height: 420 },
  });
  await context.addInitScript(mockTauri, { version: "1.0.1" });
  const page = await context.newPage();
  await page.goto(pathToFileURL(resolve("src/index.html")).href);
  await page.waitForFunction(() => !document.getElementById("toggle").disabled);
  const title = `Emploidle desktop verification ${process.pid}`;
  await page.evaluate((title) => {
    document.title = title;
  }, title);
  const handle = JSON.parse(
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `Get-Process -Name chrome | Where-Object { $_.MainWindowTitle.Contains('${title}') } | Select-Object @{Name='window';Expression={$_.MainWindowHandle.ToInt64()}}, @{Name='process_id';Expression={$_.Id}} | ConvertTo-Json -Compress`,
      ],
      { encoding: "utf8", windowsHide: true },
    ),
  );
  assert.ok(handle?.window, "The owned browser window was not found");
  const helper = readFileSync(
    "src-tauri/src/desktop/accessibility.ps1",
    "utf8",
  );
  const inputTick = () =>
    Number(
      execFileSync(
        "powershell.exe",
        [
          "-NoProfile",
          "-NonInteractive",
          "-Command",
          `
      Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NativeInput {
    [StructLayout(LayoutKind.Sequential)] private struct State { public uint Size; public uint Tick; }
    [DllImport("user32.dll")] private static extern bool GetLastInputInfo(ref State state);
    public static uint Tick() { var state = new State { Size = 8 }; if (!GetLastInputInfo(ref state)) throw new Exception("Cannot read input tick"); return state.Tick; }
}
'@
      [NativeInput]::Tick()
    `,
        ],
        { encoding: "utf8", windowsHide: true },
      ),
    );
  const request = (operation, acknowledge = true) =>
    JSON.parse(
      execFileSync(
        "powershell.exe",
        ["-NoProfile", "-NonInteractive", "-Command", helper],
        {
          input: `${JSON.stringify({ ...handle, input_tick: inputTick(), ...operation })}\n${acknowledge ? "commit\n" : ""}`,
          encoding: "utf8",
          windowsHide: true,
          timeout: 3_000,
        },
      ),
    );
  const run = (operation) => {
    const response = request(operation);
    assert.ok(response.ok, response.error);
    return response.data;
  };
  const position = () =>
    page.evaluate(() => document.scrollingElement.scrollTop);
  assert.ok(
    run({ operation: "inspect" }).scroll,
    "The app page has no restorable scroll control",
  );
  const originalPosition = await position();
  const refusedInput = request({
    operation: "scroll",
    horizontal: false,
    amount: 1,
    input_tick: (inputTick() + 1) >>> 0,
  });
  assert.equal(refusedInput.ok, false);
  assert.match(refusedInput.error, /interrupted by input/);
  assert.equal(
    await position(),
    originalPosition,
    "A stale input tick allowed scrolling",
  );
  const receipt = run({ operation: "scroll", horizontal: false, amount: 2 });
  const movedPosition = await position();
  assert.ok(
    movedPosition > originalPosition,
    "Native scrolling did not move the document",
  );
  run({ operation: "restore", receipt });
  assert.equal(
    await position(),
    originalPosition,
    "Native scrolling did not restore the document",
  );
  const unacknowledged = request(
    { operation: "scroll", horizontal: false, amount: 1 },
    false,
  );
  assert.equal(unacknowledged.ok, true);
  assert.equal(
    await position(),
    originalPosition,
    "An unacknowledged scroll was not rolled back",
  );
  const concurrent = run({ operation: "scroll", horizontal: false, amount: 1 });
  await page.evaluate(() => window.scrollBy(0, 20));
  const userPosition = await position();
  const refused = request({ operation: "restore", receipt: concurrent });
  assert.equal(refused.ok, false, "A changed view was overwritten");
  assert.match(refused.error, /preserving the current view/);
  assert.equal(await position(), userPosition);
  await page.evaluate(() => window.scrollTo(0, 0));
  const originalUrl = page.url();
  const originalTabs = context.pages().length;
  const unacknowledgedTab = request({ operation: "open_tab" }, false);
  assert.equal(unacknowledgedTab.ok, true);
  assert.equal(
    context.pages().length,
    originalTabs,
    "An unacknowledged tab was left open",
  );
  const session = run({ operation: "open_tab" });
  const selected = run({ operation: "browser_context" });
  assert.deepEqual(selected.selected, session.created);
  assert.equal(
    selected.address_focused,
    true,
    "New-tab text entry is not confined to the address bar",
  );
  run({ operation: "close_tab", session });
  await page.waitForFunction(() => document.hasFocus());
  assert.deepEqual(
    run({ operation: "browser_context" }).selected,
    session.original,
  );
  assert.equal(context.pages().length, originalTabs);
  assert.equal(page.url(), originalUrl);
  console.log(
    "PASS: native scroll restoration, concurrent-view protection, owned tab cleanup, acknowledgement rollback and original tab selection",
  );
  await context.close();
} finally {
  await browser.close();
}
