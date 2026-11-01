import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { chromium } from "playwright";
import {
  automationRequest,
  focusWindow,
  inputTick,
  readPageRequest,
} from "./lib/windows-automation.mjs";
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
  focusWindow(handle);
  const request = (operation, acknowledge = true) =>
    automationRequest(handle, operation, acknowledge);
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

  const server = createServer((request, response) => {
    response.end(`<!doctype html><title>Emploidle reading ${request.url}</title>
      <style>body{font:18px sans-serif;line-height:2;margin:30px}</style>
      <a href="/one">Own document link</a>
      ${Array.from({ length: 100 }, (_, index) => `<p>Reading fixture paragraph ${index}, with visible text for selection and restoration.</p>`).join("")}
      <script>window.positions=[];window.sawSelection=false;window.focusEvents=0;
      addEventListener('scroll',()=>positions.push(scrollY));addEventListener('focus',()=>focusEvents++);
      document.addEventListener('selectionchange',()=>{sawSelection ||= getSelection().toString().length>0});</script>`);
  });
  await new Promise((accept) => server.listen(0, "127.0.0.1", accept));
  try {
    const pages = [];
    for (const path of ["one", "two", "three"]) {
      const fixture = await context.newPage();
      await fixture.goto(`http://127.0.0.1:${server.address().port}/${path}`);
      pages.push(fixture);
    }
    assert.ok(
      run({ operation: "inspect" }).scroll,
      "Hyperlinks broke native inspection",
    );
    const catalog = run({ operation: "browser_tabs" }).tabs;
    for (const fixture of pages)
      assert.ok(catalog.some((tab) => tab.address === fixture.url()));
    const target = catalog.find((tab) => tab.address === pages[0].url());
    assert.ok(target);
    const otherBrowser = await chromium.launch({
      headless: false,
      args: ["--force-renderer-accessibility"],
    });
    try {
      const otherPage = await otherBrowser.newPage();
      await otherPage.goto(
        `http://127.0.0.1:${server.address().port}/foreground`,
      );
      const otherHandle = JSON.parse(
        execFileSync(
          "powershell.exe",
          [
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Process -Name chrome | Where-Object { $_.MainWindowTitle.Contains('Emploidle reading /foreground') } | Select-Object @{Name='window';Expression={$_.MainWindowHandle.ToInt64()}}, @{Name='process_id';Expression={$_.Id}} | ConvertTo-Json -Compress",
          ],
          { encoding: "utf8", windowsHide: true },
        ),
      );
      focusWindow(otherHandle);
      await pages[2].evaluate(() => {
        window.focusEvents = 0;
      });
      run({
        operation: "select_tab",
        tab: target,
        foreground: otherHandle.window,
      });
      focusWindow(handle);
      assert.deepEqual(
        run({ operation: "browser_context" }).selected,
        target.id,
      );
      assert.equal(
        await pages[2].evaluate(() => window.focusEvents),
        0,
        "Background selection first focused the old tab",
      );
    } finally {
      await otherBrowser.close();
    }
    const reading = pages[0];
    await reading.evaluate(() => {
      scrollTo(0, 300);
      window.positions = [];
    });
    const read = await readPageRequest(handle, reading.url(), false);
    assert.ok(read.ok, read.error);
    const result = await reading.evaluate(() => ({
      position: scrollY,
      maximum: Math.max(...window.positions),
      bottom: document.documentElement.scrollHeight - innerHeight,
      selected: window.sawSelection,
      remainingSelection: getSelection().toString(),
    }));
    assert.equal(
      result.position,
      300,
      "Reading did not restore its original scroll offset",
    );
    assert.ok(
      result.maximum >= result.bottom - 1,
      "Reading did not reach the bottom boundary",
    );
    const textReview = await readPageRequest(handle, reading.url(), true);
    assert.ok(textReview.ok, textReview.error);
    const textResult = await reading.evaluate(() => ({
      selected: window.sawSelection,
      position: scrollY,
      remainingSelection: getSelection().toString(),
    }));
    assert.ok(textResult.selected, "Visible text was never selected");
    assert.equal(
      textResult.position,
      300,
      "Text review did not restore its scroll offset",
    );
    assert.equal(
      textResult.remainingSelection,
      "",
      "Text review left a selection behind",
    );
    await reading.evaluate(() => {
      window.sawSelection = false;
    });
    let cancelText = false;
    const cancelledText = readPageRequest(handle, reading.url(), true, {
      cancel: () => cancelText,
    });
    await reading.waitForFunction(() => window.sawSelection);
    cancelText = true;
    const interruptedText = await cancelledText;
    assert.ok(interruptedText.ok, interruptedText.error);
    assert.equal(
      await reading.evaluate(() => getSelection().toString()),
      "",
      "Cancelled text review left an owned selection behind",
    );
    assert.equal(
      await reading.evaluate(() => scrollY),
      300,
      "Cancelled text review changed the original scroll offset",
    );
    assert.equal(
      result.remainingSelection,
      "",
      "Temporary text selection was left behind",
    );
    const unacknowledgedRead = await readPageRequest(
      handle,
      reading.url(),
      false,
      { acknowledge: false },
    );
    assert.ok(unacknowledgedRead.ok, unacknowledgedRead.error);
    assert.equal(
      await reading.evaluate(() => scrollY),
      300,
      "Unacknowledged reading changed the document",
    );
    let cancel = false;
    const cancelled = readPageRequest(handle, reading.url(), false, {
      cancel: () => cancel,
    });
    await reading.waitForFunction(() => scrollY > 600);
    cancel = true;
    const interrupted = await cancelled;
    assert.ok(interrupted.ok, interrupted.error);
    assert.equal(
      await reading.evaluate(() => scrollY),
      300,
      "Cancelled reading did not restore its offset",
    );
    const changedRoute = readPageRequest(handle, reading.url(), false);
    await reading.waitForFunction(() => scrollY > 600);
    await reading.evaluate(() => {
      history.pushState({}, "", "/notifications");
      scrollTo(0, 450);
    });
    const refusedRoute = await changedRoute;
    assert.equal(
      refusedRoute.ok,
      false,
      "Reading continued after the page route changed",
    );
    assert.match(
      refusedRoute.error,
      /page changed; preserving the current view/,
    );
    assert.equal(
      await reading.evaluate(() => scrollY),
      450,
      "Route-change cleanup overwrote the newer page position",
    );
    await pages[1].close();
    const removed = catalog.find((tab) => tab.address === pages[1].url());
    const refusedTab = request({
      operation: "select_tab",
      tab: removed,
      foreground: handle.window,
    });
    assert.equal(refusedTab.ok, false, "A stale tab identity was accepted");
    assert.deepEqual(run({ operation: "browser_context" }).selected, target.id);
    console.log(
      "PASS: existing-tab discovery and background selection, boundary reading, selection restoration, acknowledgement rollback, cancellation and stale-tab protection",
    );
  } finally {
    await new Promise((accept) => server.close(accept));
  }
  await context.close();
} finally {
  await browser.close();
}
